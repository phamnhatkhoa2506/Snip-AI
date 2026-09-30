//! Đọc câu trả lời thành giọng nói bằng model TTS của Gemini — "hỏi bằng
//! giọng, nghe trả lời bằng giọng" cho Snap Audio, và nút loa trên mọi câu
//! trả lời. Gemini trả PCM 16-bit thô (thường 24kHz mono) -> bọc thành WAV để
//! frontend phát thẳng bằng thẻ <audio>, không cần tự giải mã gì.
//!
//! Frontend tự rút gọn/lọc bỏ khối code, bảng... trước khi gửi (speech.ts);
//! lỗi ở đây (hết quota, mạng...) thì frontend chuyển sang giọng có sẵn của
//! Windows (speechSynthesis) thay vì im lặng.

use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::{AppHandle, Manager};

use crate::ai::{current_auth, GeminiAuth};
use crate::audio;
use crate::state::HttpClientState;

/// Chỉ dùng cho đường API key tự nhập — đường backend do server chọn model.
const DIRECT_TTS_MODEL: &str = "gemini-3.8-flash-lite-tts";
const DEFAULT_VOICE: &str = "Kore";
const MAX_CHARS: usize = 4000;
const TTS_TIMEOUT: Duration = Duration::from_secs(60);

/// "audio/L16;codec=pcm;rate=24000" -> 24000.
fn rate_from_mime(mime: &str) -> Option<u32> {
    mime.split(';').find_map(|p| p.trim().strip_prefix("rate=")).and_then(|r| r.parse().ok())
}

fn inline_audio_from_generate_response(v: &serde_json::Value) -> Option<(String, String)> {
    v["candidates"][0]["content"]["parts"].as_array()?.iter().find_map(|p| {
        let data = p["inlineData"]["data"].as_str()?;
        let mime = p["inlineData"]["mimeType"].as_str().unwrap_or("audio/L16;codec=pcm;rate=24000");
        Some((data.to_string(), mime.to_string()))
    })
}

/// Tên giọng dựng sẵn của Gemini chỉ gồm chữ cái (VD "Kore") — giá trị lạ
/// dùng giọng mặc định thay vì để API trả lỗi.
pub(crate) fn sanitize_voice(voice: Option<&str>) -> &str {
    match voice {
        Some(v) if (2..=32).contains(&v.len()) && v.chars().all(|c| c.is_ascii_alphabetic()) => v,
        _ => DEFAULT_VOICE,
    }
}

/// Tên model hợp lệ (chữ/số/`-`/`.`, bắt đầu bằng "gemini-") — chỉ dùng cho
/// đường API key tự nhập; đường backend do server tự kiểm tra danh sách.
pub(crate) fn sanitize_model<'a>(model: Option<&'a str>, fallback: &'a str) -> &'a str {
    match model {
        Some(m) if m.starts_with("gemini-") && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') => m,
        _ => fallback,
    }
}

/// Trả về WAV dạng base64. `model`/`voice`: lựa chọn trong Cài đặt -> "Giọng
/// nói AI" (thiếu thì dùng mặc định).
#[tauri::command]
pub async fn speak_text(app: AppHandle, text: String, model: Option<String>, voice: Option<String>) -> Result<String, String> {
    let text: String = text.trim().chars().take(MAX_CHARS).collect();
    if text.is_empty() {
        return Err("Không có nội dung để đọc.".into());
    }
    let voice = sanitize_voice(voice.as_deref());
    let model = sanitize_model(model.as_deref(), DIRECT_TTS_MODEL);
    let auth = current_auth()?;
    let client = &app.state::<HttpClientState>().client;

    let resp = match &auth {
        GeminiAuth::Backend { token } => client
            .post(format!("{}/v1/gemini/tts", crate::oauth::backend_base_url()))
            .bearer_auth(token)
            .json(&serde_json::json!({ "text": text, "voice": voice, "model": model }))
            .timeout(TTS_TIMEOUT)
            .send()
            .await,
        GeminiAuth::Direct { api_key } => client
            .post(format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
            ))
            .header("x-goog-api-key", api_key.as_str())
            .json(&serde_json::json!({
                "contents": [{ "parts": [{ "text": text }] }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice } } }
                }
            }))
            .timeout(TTS_TIMEOUT)
            .send()
            .await,
    }
    .map_err(|e| format!("Lỗi gửi yêu cầu đọc giọng nói: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[snip-ai][tts] HTTP {status}: {body}");
        return Err(format!("Không tạo được giọng đọc (HTTP {status})."));
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| format!("Lỗi đọc phản hồi giọng nói: {e}"))?;
    let (audio_b64, mime) = match &auth {
        GeminiAuth::Backend { .. } => (
            v["audio"].as_str().ok_or("Phản hồi thiếu dữ liệu âm thanh.")?.to_string(),
            v["mimeType"].as_str().unwrap_or("audio/L16;codec=pcm;rate=24000").to_string(),
        ),
        GeminiAuth::Direct { .. } => inline_audio_from_generate_response(&v).ok_or("Phản hồi thiếu dữ liệu âm thanh.")?,
    };

    let bytes = STANDARD.decode(audio_b64).map_err(|e| format!("Dữ liệu âm thanh hỏng: {e}"))?;
    Ok(STANDARD.encode(ensure_wav(bytes, &mime)))
}

/// Model TTS có bản trả WAV hoàn chỉnh ("audio/wav", đã có header — đo thực
/// tế với gemini-3.8-flash-lite-tts), có bản trả PCM thô ("audio/L16;...;
/// rate=24000"). Bọc thêm header lên dữ liệu ĐÃ là WAV sẽ biến 44 byte header
/// gốc thành mẫu âm thanh -> tiếng "tách" ở đầu, nên chỉ bọc khi là PCM thô.
pub(crate) fn ensure_wav(bytes: Vec<u8>, mime: &str) -> Vec<u8> {
    if bytes.starts_with(b"RIFF") {
        return bytes;
    }
    let rate = rate_from_mime(mime).unwrap_or(24_000);
    audio::wav_from_pcm16(&audio::pcm16_from_le_bytes(&bytes), rate, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_from_mime() {
        assert_eq!(rate_from_mime("audio/L16;codec=pcm;rate=24000"), Some(24_000));
        assert_eq!(rate_from_mime("audio/pcm; rate=16000"), Some(16_000));
        assert_eq!(rate_from_mime("audio/pcm"), None);
    }

    #[test]
    fn keeps_wav_and_wraps_raw_pcm() {
        let wav = audio::wav_from_pcm16(&[1, 2], 24_000, 1);
        assert_eq!(ensure_wav(wav.clone(), "audio/wav"), wav);
        let raw: Vec<u8> = [5i16, -5].iter().flat_map(|s| s.to_le_bytes()).collect();
        let wrapped = ensure_wav(raw, "audio/L16;codec=pcm;rate=16000");
        assert_eq!(&wrapped[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wrapped[24..28].try_into().unwrap()), 16_000);
        assert_eq!(audio::pcm16_from_le_bytes(&wrapped[44..]), vec![5, -5]);
    }

    #[test]
    fn extracts_inline_audio() {
        let v = serde_json::json!({"candidates":[{"content":{"parts":[{"text":"x"},{"inlineData":{"mimeType":"audio/L16;rate=24000","data":"AAA="}}]}}]});
        assert_eq!(
            inline_audio_from_generate_response(&v),
            Some(("AAA=".to_string(), "audio/L16;rate=24000".to_string()))
        );
    }
}
