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

/// Trả về WAV dạng base64.
#[tauri::command]
pub async fn speak_text(app: AppHandle, text: String) -> Result<String, String> {
    let text: String = text.trim().chars().take(MAX_CHARS).collect();
    if text.is_empty() {
        return Err("Không có nội dung để đọc.".into());
    }
    let auth = current_auth()?;
    let client = &app.state::<HttpClientState>().client;

    let resp = match &auth {
        GeminiAuth::Backend { token } => client
            .post(format!("{}/v1/gemini/tts", crate::oauth::backend_base_url()))
            .bearer_auth(token)
            .json(&serde_json::json!({ "text": text, "voice": DEFAULT_VOICE }))
            .timeout(TTS_TIMEOUT)
            .send()
            .await,
        GeminiAuth::Direct { api_key } => client
            .post(format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{DIRECT_TTS_MODEL}:generateContent"
            ))
            .header("x-goog-api-key", api_key.as_str())
            .json(&serde_json::json!({
                "contents": [{ "parts": [{ "text": text }] }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": DEFAULT_VOICE } } }
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

    let pcm = STANDARD.decode(audio_b64).map_err(|e| format!("Dữ liệu âm thanh hỏng: {e}"))?;
    let rate = rate_from_mime(&mime).unwrap_or(24_000);
    let wav = audio::wav_from_pcm16(&audio::pcm16_from_le_bytes(&pcm), rate, 1);
    Ok(STANDARD.encode(wav))
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
    fn extracts_inline_audio() {
        let v = serde_json::json!({"candidates":[{"content":{"parts":[{"text":"x"},{"inlineData":{"mimeType":"audio/L16;rate=24000","data":"AAA="}}]}}]});
        assert_eq!(
            inline_audio_from_generate_response(&v),
            Some(("AAA=".to_string(), "audio/L16;rate=24000".to_string()))
        );
    }
}
