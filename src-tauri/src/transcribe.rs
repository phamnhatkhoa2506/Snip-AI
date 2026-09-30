//! Chép lời 1 đoạn ghi âm (Snap Audio) bằng `gemini-3.5-transcribe` — model
//! chuyên chép lời (nhận 85+ ngôn ngữ, tự nhận biết ngôn ngữ từng câu, tách
//! người nói, mốc thời gian từng từ), nhanh và rẻ hơn hẳn việc nhờ model chat
//! "chép lời" qua câu lệnh, và sát NGUYÊN VĂN hơn (model chat hay tóm tắt/sửa
//! chữ). Gọi qua Interactions API (khác generateContent), xem geminiProxy.ts.
//!
//! Trả về bản chép đã định dạng Markdown: mỗi đoạn mở đầu bằng mốc `[mm:ss]`
//! (bấm để tua, xem linkifyTimestamps) và nhãn người nói nếu có từ 2 người.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::ai::{current_auth, GeminiAuth};
use crate::state::{AppState, HttpClientState, MediaKind};

const TRANSCRIBE_MODEL: &str = "gemini-3.5-transcribe";
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(180);
/// Im lặng dài hơn mức này giữa 2 từ -> tách thành đoạn mới.
const PAUSE_SPLIT_SECS: f64 = 1.2;
/// Đoạn dài quá mức này thì cắt (bản chép dễ đọc hơn, mốc thời gian dày hơn).
const MAX_SEGMENT_SECS: f64 = 25.0;

#[derive(Serialize, Debug, PartialEq)]
pub struct Segment {
    pub speaker: Option<String>,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Serialize, Debug)]
pub struct TranscriptResult {
    /// Bản chép Markdown (mốc `[mm:ss]`, nhãn người nói) — hiện thẳng trong chat.
    pub markdown: String,
    /// Văn bản thuần, không mốc/nhãn — để chép vào clipboard/xuất file.
    pub text: String,
    pub segments: Vec<Segment>,
}

/// "0.450s" -> 0.45
fn parse_offset(s: &str) -> Option<f64> {
    s.trim().strip_suffix('s').unwrap_or(s).trim().parse().ok()
}

/// Gom các `word_info` thành đoạn theo người nói / khoảng lặng / độ dài.
fn build_segments(resp: &serde_json::Value) -> (String, Vec<Segment>) {
    let mut plain = String::new();
    let mut segs: Vec<Segment> = Vec::new();

    for step in resp["steps"].as_array().into_iter().flatten() {
        for item in step["content"].as_array().into_iter().flatten() {
            if item["type"] != "text" {
                continue;
            }
            let item_text = item["text"].as_str().unwrap_or("");
            if !plain.is_empty() && !item_text.is_empty() {
                plain.push(' ');
            }
            plain.push_str(item_text.trim());

            let mut any_word = false;
            for ann in item["annotations"].as_array().into_iter().flatten() {
                if ann["type"] != "word_info" {
                    continue;
                }
                let Some(word) = ann["text"].as_str().filter(|w| !w.is_empty()) else { continue };
                let start = ann["start_offset"].as_str().and_then(parse_offset).unwrap_or(0.0);
                let end = ann["end_offset"].as_str().and_then(parse_offset).unwrap_or(start);
                let speaker = ann["speaker"].as_str().map(str::to_string);
                any_word = true;

                let split = match segs.last() {
                    None => true,
                    Some(last) => {
                        last.speaker != speaker || start - last.end > PAUSE_SPLIT_SECS || end - last.start > MAX_SEGMENT_SECS
                    }
                };
                if split {
                    segs.push(Segment { speaker, start, end, text: word.to_string() });
                } else if let Some(last) = segs.last_mut() {
                    last.text.push(' ');
                    last.text.push_str(word);
                    last.end = end;
                }
            }
            // Không có thông tin từng từ (chế độ smart, hoặc Google bỏ qua) ->
            // dùng nguyên khối văn bản làm 1 đoạn, không mốc thời gian.
            if !any_word && !item_text.trim().is_empty() {
                segs.push(Segment { speaker: None, start: 0.0, end: 0.0, text: item_text.trim().to_string() });
            }
        }
    }
    (plain, segs)
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0).floor() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// "spk_1" -> "Người 1" (đánh số lại theo thứ tự xuất hiện để luôn là 1, 2, 3...).
fn format_markdown(segs: &[Segment]) -> String {
    let mut order: Vec<&str> = Vec::new();
    for s in segs {
        if let Some(sp) = s.speaker.as_deref() {
            if !order.contains(&sp) {
                order.push(sp);
            }
        }
    }
    let show_speakers = order.len() >= 2;
    let has_timing = segs.iter().any(|s| s.end > 0.0);

    segs.iter()
        .map(|s| {
            let mut line = String::new();
            if has_timing {
                line.push_str(&format!("[{}] ", clock(s.start)));
            }
            if show_speakers {
                if let Some(pos) = s.speaker.as_deref().and_then(|sp| order.iter().position(|o| *o == sp)) {
                    line.push_str(&format!("**Người {}:** ", pos + 1));
                }
            }
            line.push_str(&s.text);
            line
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn parse_response(resp: &serde_json::Value) -> Result<TranscriptResult, String> {
    let (text, segments) = build_segments(resp);
    if text.trim().is_empty() && segments.is_empty() {
        return Err("Không nghe thấy lời nói nào trong đoạn ghi âm.".into());
    }
    let markdown = format_markdown(&segments);
    // Ghép văn bản thuần từ CÁC ĐOẠN (cách nhau 1 dấu cách) — trường `text`
    // gốc của Google dính liền câu cuối người này với câu đầu người kia
    // ("...nhân tạo.Cảm ơn thầy").
    let text = if segments.is_empty() {
        text
    } else {
        segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
    };
    Ok(TranscriptResult { markdown, text, segments })
}

/// Cấu hình chép lời: `verbatim` + tách người nói + mốc thời gian từng từ
/// (đoạn ghi âm của app ≤ 2 phút nên trong giới hạn 30 phút của 2 tính năng
/// này). `language` là mã BCP-47 (VD "vi-VN"); không truyền = tự nhận biết.
pub(crate) fn transcription_config(language: Option<&str>) -> serde_json::Value {
    let mut cfg = serde_json::json!({
        "mode": { "type": "verbatim", "diarization_mode": "speaker", "timestamp_granularities": ["word"] }
    });
    if let Some(l) = language.filter(|l| (2..=12).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        cfg["language_codes"] = serde_json::json!([l]);
    }
    cfg
}

/// Đoạn ghi âm: phần tử `index` trong chuỗi media của phiên (mặc định: đoạn
/// audio MỚI NHẤT).
fn audio_bytes(state: &State<'_, AppState>, window_label: &str, index: Option<usize>) -> Result<Vec<u8>, String> {
    let sessions = state.media_sessions.lock().unwrap();
    let list = sessions.get(window_label).ok_or("Không tìm thấy phiên này")?;
    let item = match index {
        Some(i) => list.get(i),
        None => list.iter().rev().find(|m| m.kind == MediaKind::Audio),
    };
    match item {
        Some(m) if m.kind == MediaKind::Audio => Ok(m.bytes.clone()),
        _ => Err("Phiên này chưa có đoạn ghi âm nào để chép lời.".into()),
    }
}

#[tauri::command]
pub async fn transcribe_session_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    index: Option<usize>,
    language: Option<String>,
) -> Result<TranscriptResult, String> {
    let bytes = audio_bytes(&state, &window_label, index)?;
    let auth = current_auth()?;
    let client = &app.state::<HttpClientState>().client;
    let config = transcription_config(language.as_deref());

    let resp = match &auth {
        GeminiAuth::Backend { token } => {
            let cfg = url::form_urlencoded::byte_serialize(config.to_string().as_bytes()).collect::<String>();
            client
                .post(format!("{}/v1/gemini/transcribe", crate::oauth::backend_base_url()))
                .bearer_auth(token)
                .header("x-gemini-mime", "audio/wav")
                .header("x-transcribe-config", cfg)
                .body(bytes)
                .timeout(TRANSCRIBE_TIMEOUT)
                .send()
                .await
        }
        GeminiAuth::Direct { api_key } => {
            let up = crate::file_api::upload_file(client, &auth, "audio/wav", "snap-ai-audio", &bytes, None).await?;
            client
                .post("https://generativelanguage.googleapis.com/v1beta/interactions")
                .header("x-goog-api-key", api_key.as_str())
                .json(&serde_json::json!({
                    "model": TRANSCRIBE_MODEL,
                    "input": [{ "type": "audio", "uri": up.uri, "mime_type": "audio/wav" }],
                    "generation_config": { "transcription_config": config },
                }))
                .timeout(TRANSCRIBE_TIMEOUT)
                .send()
                .await
        }
    }
    .map_err(|e| format!("Lỗi gửi yêu cầu chép lời: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        eprintln!("[snip-ai][transcribe] HTTP {status}: {text}");
        return Err(format!("Không chép được lời (HTTP {status})."));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("Lỗi đọc kết quả chép lời: {e}"))?;
    parse_response(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, speaker: &str, start: &str, end: &str) -> serde_json::Value {
        serde_json::json!({"type":"word_info","text":text,"speaker":speaker,"start_offset":start,"end_offset":end})
    }

    #[test]
    fn groups_words_by_speaker_pause_and_formats_markdown() {
        let resp = serde_json::json!({"steps":[{"type":"model_output","content":[{
            "type":"text","text":"Xin chào các bạn Chào bác",
            "annotations":[
                word("Xin","spk_1","0.100s","0.300s"), word("chào","spk_1","0.300s","0.600s"), word("các","spk_1","0.600s","0.800s"),
                word("bạn","spk_1","0.800s","1.100s"),
                word("Chào","spk_2","1.300s","1.600s"), word("bác","spk_2","1.600s","1.900s"),
                word("Hôm","spk_2","9.000s","9.300s"), // nghỉ dài -> đoạn mới cùng người nói
            ]}]}]});
        let r = parse_response(&resp).unwrap();
        let got: Vec<(&str, f64, &str)> =
            r.segments.iter().map(|s| (s.speaker.as_deref().unwrap(), s.start, s.text.as_str())).collect();
        assert_eq!(got, vec![("spk_1", 0.1, "Xin chào các bạn"), ("spk_2", 1.3, "Chào bác"), ("spk_2", 9.0, "Hôm")]);
        assert_eq!(r.markdown, "[00:00] **Người 1:** Xin chào các bạn\n\n[00:01] **Người 2:** Chào bác\n\n[00:09] **Người 2:** Hôm");
        assert_eq!(r.text, "Xin chào các bạn Chào bác Hôm");
    }

    #[test]
    fn single_speaker_has_no_speaker_labels() {
        let resp = serde_json::json!({"steps":[{"content":[{"type":"text","text":"a b","annotations":[
            word("a","spk_1","0.0s","0.2s"), word("b","spk_1","0.2s","0.4s")]}]}]});
        assert_eq!(parse_response(&resp).unwrap().markdown, "[00:00] a b");
    }

    #[test]
    fn falls_back_to_plain_text_without_word_info() {
        let resp = serde_json::json!({"steps":[{"content":[{"type":"text","text":"Chỉ có chữ thôi"}]}]});
        let r = parse_response(&resp).unwrap();
        assert_eq!(r.markdown, "Chỉ có chữ thôi");
    }

    #[test]
    fn empty_result_is_a_clear_error() {
        let resp = serde_json::json!({"steps":[{"content":[{"type":"text","text":"  "}]}]});
        assert!(parse_response(&resp).unwrap_err().contains("Không nghe thấy"));
    }

    #[test]
    fn config_has_diarization_timestamps_and_optional_language() {
        let c = transcription_config(None);
        assert_eq!(c["mode"]["diarization_mode"], "speaker");
        assert_eq!(c["mode"]["timestamp_granularities"][0], "word");
        assert!(c["language_codes"].is_null());
        assert_eq!(transcription_config(Some("vi-VN"))["language_codes"][0], "vi-VN");
        assert!(transcription_config(Some("vi; drop"))["language_codes"].is_null());
    }

    #[test]
    fn offsets_parse() {
        assert_eq!(parse_offset("0.450s"), Some(0.45));
        assert_eq!(parse_offset("12s"), Some(12.0));
        assert_eq!(parse_offset("x"), None);
    }
}
