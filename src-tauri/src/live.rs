//! Trò chuyện TRỰC TIẾP bằng giọng nói với Gemini Live API (WebSocket 2
//! chiều): micro -> PCM 16kHz gửi lên liên tục, Gemini tự nhận biết lúc người
//! dùng nói xong và trả lời bằng giọng (PCM 24kHz) phát thẳng ra loa, kèm
//! lời thoại 2 phía (transcription) hiện lên cửa sổ "live". Người dùng nói
//! chen ngang thì Gemini báo `interrupted` -> bỏ ngay phần giọng AI chưa phát.
//!
//! Toàn bộ âm thanh xử lý ở Rust (thu qua audio.rs, phát qua `Playback`) —
//! WebView chỉ hiển thị trạng thái, không đụng tới micro/loa, nên quyền micro
//! vẫn đi đúng 1 đường (WASAPI) như Snap Audio.
//!
//! CHỐNG TIẾNG VỌNG: thu âm WASAPI không có khử tiếng vọng — dùng loa ngoài
//! thì micro nghe lại chính giọng AI rồi gửi ngược lên (AI tự ngắt lời mình).
//! Mặc định: lúc AI đang nói (và ~300ms sau đó) gửi IM LẶNG thay cho tiếng
//! micro — đổi lại không nói chen ngang được. Bật "Đang dùng tai nghe" thì
//! gửi thẳng tiếng micro, chen ngang tự nhiên như gọi điện thật.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use crate::ai::{current_auth, GeminiAuth};
use crate::audio::{self, AudioSource, CaptureOptions, Playback};
use crate::state::AppState;

pub const LIVE_LABEL: &str = "live";
/// Chỉ dùng cho đường API key tự nhập — đường backend do server chọn model.
const DIRECT_LIVE_MODEL: &str = "gemini-3.8-live";
const INPUT_RATE: u32 = 16_000;
const OUTPUT_RATE_DEFAULT: u32 = 24_000;
/// Nhịp gửi tiếng micro — đủ nhanh cho hội thoại tự nhiên, không quá dày
/// (mỗi tin nhắn đi qua Durable Object đều bị tính 1 phần request).
const FRAME_MS: u32 = 100;
const MAX_SESSION: Duration = Duration::from_secs(10 * 60);
const ECHO_TAIL: Duration = Duration::from_millis(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

const LIVE_SYSTEM_PROMPT: &str = "\
Bạn là trợ lý AI của app Snap AI, đang TRÒ CHUYỆN BẰNG GIỌNG NÓI trực tiếp với người dùng. \
Trả lời bằng đúng ngôn ngữ người dùng đang nói (mặc định tiếng Việt), tự nhiên như nói chuyện, \
ngắn gọn — vài câu là đủ, trừ khi được hỏi chi tiết. KHÔNG dùng Markdown, ký hiệu, bảng, khối \
code hay đọc to đường link — mọi thứ phải nghe được. Không chắc thì nói thẳng là không chắc, \
không bịa. Nghe không rõ thì hỏi lại.";

pub enum LiveCommand {
    Stop,
}

pub struct LiveSession {
    commands: mpsc::UnboundedSender<LiveCommand>,
    muted: Arc<AtomicBool>,
    headphones: Arc<AtomicBool>,
}

#[derive(Serialize, Clone)]
struct TranscriptPayload {
    role: &'static str,
    text: String,
}

/// Mở (hoặc đưa lên trước) cửa sổ trò chuyện trực tiếp — 1 cửa sổ duy nhất.
#[tauri::command]
pub async fn open_live_window(app: AppHandle) -> Result<(), String> {
    if !crate::oauth::is_logged_in() {
        crate::commands::show_settings_window(app.clone());
        return Err("Cần đăng nhập Google trước khi dùng AI.".into());
    }
    if let Some(win) = app.get_webview_window(LIVE_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let (mon_x, mon_y, mon_w, mon_h, scale) = crate::commands::primary_monitor_rect(&app);
    let win_w = (380.0_f64 * scale).round();
    let win_h = (560.0_f64 * scale).round();
    let pos_x = mon_x as f64 + (mon_w as f64 - win_w) / 2.0;
    let pos_y = mon_y as f64 + (mon_h as f64 - win_h) / 2.0;
    let win = WebviewWindowBuilder::new(&app, LIVE_LABEL, WebviewUrl::App("live".into()))
        .title("Trò chuyện trực tiếp")
        .decorations(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được cửa sổ trò chuyện: {e}"))?;
    let _ = win.set_size(PhysicalSize::new(win_w, win_h));
    let _ = win.set_min_size(Some(PhysicalSize::new(320.0 * scale, 420.0 * scale)));
    let _ = win.set_position(PhysicalPosition::new(pos_x.max(mon_x as f64), pos_y.max(mon_y as f64)));
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

fn setup_message(model: &str, voice: &str) -> serde_json::Value {
    let mut setup = serde_json::json!({
        "setup": {
            // Đường backend: server ghi đè bằng model đã kiểm tra theo danh
            // sách cho phép (header x-snap-model, xem geminiProxy.ts/index.ts).
            "model": format!("models/{model}"),
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice } } }
            },
            "systemInstruction": { "parts": [{ "text": LIVE_SYSTEM_PROMPT }] },
            "inputAudioTranscription": {},
            "outputAudioTranscription": {}
        }
    });
    // Chỉ bản "extended thinking" BẮT BUỘC khai báo mức suy luận (server đóng
    // phiên nếu thiếu); bản thường thì NGƯỢC LẠI — phải bỏ field này.
    if model.ends_with("-extended-thinking") {
        setup["setup"]["generationConfig"]["thinkingConfig"] = serde_json::json!({ "thinkingLevel": "low" });
    }
    setup
}

fn audio_message(samples: &[i16]) -> String {
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    serde_json::json!({
        "realtimeInput": { "audio": { "data": STANDARD.encode(bytes), "mimeType": "audio/pcm;rate=16000" } }
    })
    .to_string()
}

/// "audio/pcm;rate=24000" -> 24000.
fn rate_from_mime(mime: &str) -> Option<u32> {
    mime.split(';').find_map(|p| p.trim().strip_prefix("rate=")).and_then(|r| r.parse().ok())
}

/// Những gì 1 tin nhắn từ server yêu cầu làm — tách riêng để test được mà
/// không cần mạng/loa.
#[derive(Debug, Default, PartialEq)]
struct ServerEvents {
    audio: Vec<(Vec<i16>, u32)>,
    user_text: Option<String>,
    model_text: Option<String>,
    interrupted: bool,
    turn_complete: bool,
    setup_complete: bool,
    go_away: bool,
}

fn parse_server_message(raw: &str) -> ServerEvents {
    let mut ev = ServerEvents::default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { return ev };
    ev.setup_complete = v.get("setupComplete").is_some();
    ev.go_away = v.get("goAway").is_some();
    let sc = &v["serverContent"];
    if let Some(parts) = sc["modelTurn"]["parts"].as_array() {
        for p in parts {
            let Some(data) = p["inlineData"]["data"].as_str() else { continue };
            let rate = p["inlineData"]["mimeType"].as_str().and_then(rate_from_mime).unwrap_or(OUTPUT_RATE_DEFAULT);
            if let Ok(bytes) = STANDARD.decode(data) {
                ev.audio.push((audio::pcm16_from_le_bytes(&bytes), rate));
            }
        }
    }
    ev.user_text = sc["inputTranscription"]["text"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    ev.model_text = sc["outputTranscription"]["text"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    ev.interrupted = sc["interrupted"].as_bool().unwrap_or(false);
    ev.turn_complete = sc["turnComplete"].as_bool().unwrap_or(false);
    ev
}

fn message_text(msg: &Message) -> Option<String> {
    match msg {
        Message::Text(t) => Some(t.as_str().to_string()),
        // Google gửi JSON dưới dạng khung BINARY — vẫn là UTF-8.
        Message::Binary(b) => String::from_utf8(b.to_vec()).ok(),
        _ => None,
    }
}

fn describe_connect_error(e: &tokio_tungstenite::tungstenite::Error, via_backend: bool) -> String {
    use tokio_tungstenite::tungstenite::Error as WsError;
    match e {
        WsError::Http(resp) => match resp.status().as_u16() {
            401 => "Phiên đăng nhập đã hết hạn — đăng xuất rồi đăng nhập lại.".into(),
            404 | 426 if via_backend => "Máy chủ của app chưa hỗ trợ trò chuyện trực tiếp (cần cập nhật backend).".into(),
            s => format!("Máy chủ từ chối kết nối trò chuyện trực tiếp (HTTP {s})."),
        },
        other => format!("Không kết nối được trò chuyện trực tiếp: {other}"),
    }
}

/// `model`/`voice`: lựa chọn trong Cài đặt -> "Giọng nói AI" (thiếu thì dùng
/// mặc định).
#[tauri::command]
pub async fn live_start(app: AppHandle, headphones: bool, model: Option<String>, voice: Option<String>) -> Result<(), String> {
    let model = crate::tts::sanitize_model(model.as_deref(), DIRECT_LIVE_MODEL).to_string();
    let voice = crate::tts::sanitize_voice(voice.as_deref()).to_string();
    {
        let state = app.state::<AppState>();
        if state.live.lock().unwrap().is_some() {
            return Err("Đang có 1 phiên trò chuyện trực tiếp chạy rồi.".into());
        }
    }

    let auth = current_auth()?;
    let via_backend = matches!(auth, GeminiAuth::Backend { .. });
    let mut request = match &auth {
        GeminiAuth::Backend { .. } => {
            let base = crate::oauth::backend_base_url().replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
            format!("{base}/v1/gemini/live")
        }
        GeminiAuth::Direct { api_key } => format!(
            "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={api_key}"
        ),
    }
    .into_client_request()
    .map_err(|e| format!("Địa chỉ kết nối không hợp lệ: {e}"))?;
    if let GeminiAuth::Backend { token } = &auth {
        let value = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|e| format!("Token không hợp lệ: {e}"))?;
        request.headers_mut().insert("Authorization", value);
        // `model` đã qua sanitize_model — chỉ còn ký tự hợp lệ cho header.
        if let Ok(value) = HeaderValue::from_str(&model) {
            request.headers_mut().insert("x-snap-model", value);
        }
    }

    let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
        .await
        .map_err(|_| "Kết nối trò chuyện trực tiếp quá lâu, thử lại sau.".to_string())?
        .map_err(|e| describe_connect_error(&e, via_backend))?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    ws_tx
        .send(Message::text(setup_message(&model, &voice).to_string()))
        .await
        .map_err(|e| format!("Lỗi gửi cấu hình phiên: {e}"))?;

    // Chờ server xác nhận setup trước khi mở micro — setup sai (model không
    // tồn tại, bị chặn vùng...) thì server đóng kết nối ngay, báo lỗi rõ ràng
    // thay vì mở micro rồi im lặng.
    let wait_setup = async {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Close(frame)) => {
                    let reason = frame.map(|f| f.reason.as_str().to_string()).unwrap_or_default();
                    return Err(format!("Gemini đóng phiên ngay khi bắt đầu: {reason}"));
                }
                Ok(m) => {
                    if message_text(&m).is_some_and(|t| parse_server_message(&t).setup_complete) {
                        return Ok(());
                    }
                }
                Err(e) => return Err(format!("Mất kết nối khi khởi tạo phiên: {e}")),
            }
        }
        Err("Kết nối bị đóng khi khởi tạo phiên.".to_string())
    };
    tokio::time::timeout(CONNECT_TIMEOUT, wait_setup)
        .await
        .map_err(|_| "Gemini không phản hồi khi khởi tạo phiên.".to_string())??;

    let playback = Playback::start()?;
    let (mic_tx, mut mic_rx) = mpsc::unbounded_channel::<Vec<i16>>();
    let level_app = app.clone();
    let (capture, _) = tauri::async_runtime::spawn_blocking(move || {
        audio::start_capture(
            CaptureOptions { sources: vec![AudioSource::Mic], sample_rate: INPUT_RATE, channels: 1, frame_ms: FRAME_MS, strict: true },
            move |frame| {
                let _ = level_app.emit_to(LIVE_LABEL, "live:level", audio::level(frame));
                let _ = mic_tx.send(frame.to_vec());
            },
        )
    })
    .await
    .map_err(|e| format!("Lỗi nội bộ khi mở micro: {e}"))??;

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<LiveCommand>();
    let muted = Arc::new(AtomicBool::new(false));
    let headphones_flag = Arc::new(AtomicBool::new(headphones));
    // Cửa sổ có thể đã bị đóng trong lúc đang kết nối (lệnh dừng lúc đó chưa
    // có phiên nào để dừng) — không kiểm tra thì phiên chạy ngầm, giữ micro
    // mở tới 10 phút mà không có cửa sổ nào hiển thị.
    if app.get_webview_window(LIVE_LABEL).is_none() {
        let _ = cmd_tx.send(LiveCommand::Stop);
    }
    *app.state::<AppState>().live.lock().unwrap() =
        Some(LiveSession { commands: cmd_tx, muted: muted.clone(), headphones: headphones_flag.clone() });
    let _ = app.emit_to(LIVE_LABEL, "live:state", "listening");

    let loop_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let app = loop_app;
        let deadline = tokio::time::sleep(MAX_SESSION);
        tokio::pin!(deadline);
        let silence = vec![0i16; (INPUT_RATE * FRAME_MS / 1000) as usize];
        let mut model_speaking = false;

        let end_reason: String = loop {
            tokio::select! {
                _ = &mut deadline => break "Đã hết 10 phút của phiên trò chuyện.".into(),
                cmd = cmd_rx.recv() => match cmd {
                    Some(LiveCommand::Stop) | None => break String::new(),
                },
                frame = mic_rx.recv() => {
                    let Some(frame) = frame else { break "Micro đã dừng.".into() };
                    let gated = muted.load(Ordering::SeqCst)
                        || (!headphones_flag.load(Ordering::SeqCst) && playback.is_active(ECHO_TAIL));
                    let payload = audio_message(if gated { &silence } else { &frame });
                    if ws_tx.send(Message::text(payload)).await.is_err() {
                        break "Mất kết nối tới máy chủ.".into();
                    }
                    if model_speaking && !playback.is_active(Duration::ZERO) {
                        model_speaking = false;
                        let _ = app.emit_to(LIVE_LABEL, "live:state", "listening");
                    }
                },
                msg = ws_rx.next() => match msg {
                    Some(Ok(Message::Close(frame))) => {
                        break frame.map(|f| f.reason.as_str().to_string()).filter(|r| !r.is_empty())
                            .unwrap_or_else(|| "Phiên đã kết thúc.".into());
                    }
                    Some(Ok(m)) => {
                        let Some(text) = message_text(&m) else { continue };
                        let ev = parse_server_message(&text);
                        if ev.interrupted {
                            playback.clear();
                            let _ = app.emit_to(LIVE_LABEL, "live:interrupted", ());
                        }
                        for (samples, rate) in &ev.audio {
                            playback.push_pcm16(samples, *rate);
                        }
                        if !ev.audio.is_empty() && !model_speaking {
                            model_speaking = true;
                            let _ = app.emit_to(LIVE_LABEL, "live:state", "speaking");
                        }
                        if let Some(t) = ev.user_text {
                            let _ = app.emit_to(LIVE_LABEL, "live:transcript", TranscriptPayload { role: "user", text: t });
                        }
                        if let Some(t) = ev.model_text {
                            let _ = app.emit_to(LIVE_LABEL, "live:transcript", TranscriptPayload { role: "model", text: t });
                        }
                        if ev.turn_complete {
                            let _ = app.emit_to(LIVE_LABEL, "live:turn-complete", ());
                        }
                        if ev.go_away {
                            let _ = app.emit_to(LIVE_LABEL, "live:notice", "Phiên sắp bị máy chủ kết thúc.");
                        }
                    }
                    Some(Err(e)) => break format!("Mất kết nối: {e}"),
                    None => break "Máy chủ đã đóng kết nối.".into(),
                },
            }
        };

        capture.stop();
        playback.clear();
        let _ = ws_tx.send(Message::Close(None)).await;
        *app.state::<AppState>().live.lock().unwrap() = None;
        let _ = app.emit_to(LIVE_LABEL, "live:ended", end_reason);
    });

    Ok(())
}

#[tauri::command]
pub fn live_stop(state: State<'_, AppState>) {
    if let Some(session) = state.live.lock().unwrap().as_ref() {
        let _ = session.commands.send(LiveCommand::Stop);
    }
}

#[tauri::command]
pub fn live_set_muted(state: State<'_, AppState>, muted: bool) {
    if let Some(session) = state.live.lock().unwrap().as_ref() {
        session.muted.store(muted, Ordering::SeqCst);
    }
}

#[tauri::command]
pub fn live_set_headphones(state: State<'_, AppState>, headphones: bool) {
    if let Some(session) = state.live.lock().unwrap().as_ref() {
        session.headphones.store(headphones, Ordering::SeqCst);
    }
}

/// Cửa sổ "live" bị đóng — dừng phiên (nhả micro, đóng kết nối).
pub fn on_window_destroyed(app: &AppHandle) {
    let state = app.state::<AppState>();
    let guard = state.live.lock().unwrap();
    if let Some(session) = guard.as_ref() {
        let _ = session.commands.send(LiveCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_audio_transcripts_and_flags() {
        let pcm: Vec<u8> = [100i16, -100].iter().flat_map(|s| s.to_le_bytes()).collect();
        let raw = serde_json::json!({
            "serverContent": {
                "modelTurn": { "parts": [{ "inlineData": { "mimeType": "audio/pcm;rate=24000", "data": STANDARD.encode(&pcm) } }] },
                "inputTranscription": { "text": "xin chào" },
                "outputTranscription": { "text": "chào bạn" },
                "turnComplete": true
            }
        })
        .to_string();
        let ev = parse_server_message(&raw);
        assert_eq!(ev.audio, vec![(vec![100, -100], 24_000)]);
        assert_eq!(ev.user_text.as_deref(), Some("xin chào"));
        assert_eq!(ev.model_text.as_deref(), Some("chào bạn"));
        assert!(ev.turn_complete && !ev.interrupted && !ev.setup_complete);
    }

    #[test]
    fn parses_setup_complete_and_interrupted() {
        assert!(parse_server_message(r#"{"setupComplete":{}}"#).setup_complete);
        assert!(parse_server_message(r#"{"serverContent":{"interrupted":true}}"#).interrupted);
        assert_eq!(parse_server_message("không phải json"), ServerEvents::default());
    }

    #[test]
    fn audio_message_is_valid_realtime_input() {
        let v: serde_json::Value = serde_json::from_str(&audio_message(&[1, 2, 3])).unwrap();
        assert_eq!(v["realtimeInput"]["audio"]["mimeType"], "audio/pcm;rate=16000");
        let data = STANDARD.decode(v["realtimeInput"]["audio"]["data"].as_str().unwrap()).unwrap();
        assert_eq!(audio::pcm16_from_le_bytes(&data), vec![1, 2, 3]);
    }

    // ── Test đầu-cuối với backend PRODUCTION, dùng phiên đăng nhập Google
    // đang lưu trên máy (Credential Manager) — `cargo test -- --ignored e2e`.
    // Không in token ra; tốn vài lượt quota free-tier mỗi lần chạy.

    fn e2e_token() -> String {
        crate::oauth::read_session_token().expect("máy này chưa đăng nhập app")
    }

    #[test]
    #[ignore]
    fn e2e_chat_uses_selected_model_and_rejects_unknown() {
        tauri::async_runtime::block_on(async {
            let client = reqwest::Client::new();
            let body = serde_json::json!({"contents":[{"role":"user","parts":[{"text":"Trả lời đúng 1 từ: OK"}]}]});
            for (asked, expected) in [("gemini-3.8-flash", "gemini-3.8-flash"), ("gemini-9-ultra-hack", "gemini-3.6-flash")] {
                let resp = client
                    .post(format!("{}/v1/gemini/stream", crate::oauth::backend_base_url()))
                    .bearer_auth(e2e_token())
                    .header("x-snap-model", asked)
                    .json(&body)
                    .send()
                    .await
                    .unwrap();
                let status = resp.status();
                let text = resp.text().await.unwrap();
                assert!(status.is_success(), "{asked}: HTTP {status}: {text}");
                let version = text
                    .split("\"modelVersion\":")
                    .nth(1)
                    .and_then(|s| s.split('"').nth(1))
                    .unwrap_or("")
                    .to_string();
                println!("hỏi {asked} -> server dùng {version}");
                assert!(version.starts_with(expected), "{asked}: server dùng {version}, mong đợi {expected}");
            }
        });
    }

    #[test]
    #[ignore]
    fn e2e_tts_returns_audio() {
        tauri::async_runtime::block_on(async {
            let resp = reqwest::Client::new()
                .post(format!("{}/v1/gemini/tts", crate::oauth::backend_base_url()))
                .bearer_auth(e2e_token())
                .json(&serde_json::json!({"text": "Xin chào, đây là thử giọng đọc."}))
                .send()
                .await
                .unwrap();
            let status = resp.status();
            let v: serde_json::Value = resp.json().await.unwrap();
            assert!(status.is_success(), "HTTP {status}: {v}");
            let raw = STANDARD.decode(v["audio"].as_str().expect("thiếu audio")).unwrap();
            let mime = v["mimeType"].as_str().unwrap_or("").to_string();
            let wav = crate::tts::ensure_wav(raw, &mime);
            assert_eq!(&wav[0..4], b"RIFF", "không ra WAV hợp lệ");
            let rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
            let bits = u16::from_le_bytes(wav[34..36].try_into().unwrap());
            println!("TTS: mime {mime}, WAV {} bytes, {rate}Hz {bits}-bit", wav.len());
            assert!(wav.len() > 24_000, "âm thanh quá ngắn");
        });
    }

    #[test]
    #[ignore]
    fn e2e_live_full_turn_via_backend() {
        for (model, voice) in [("gemini-3.8-live", "Puck"), ("gemini-3.8-live-extended-thinking", "Sulafat")] {
            println!("== {model} / {voice}");
            e2e_live_one_turn(model, voice);
        }
    }

    fn e2e_live_one_turn(model: &str, voice: &str) {
        tauri::async_runtime::block_on(async {
            let url = format!("{}/v1/gemini/live", crate::oauth::backend_base_url().replacen("https://", "wss://", 1));
            let mut req = url.into_client_request().unwrap();
            req.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", e2e_token())).unwrap());
            req.headers_mut().insert("x-snap-model", HeaderValue::from_str(model).unwrap());
            let (ws, _) = tokio_tungstenite::connect_async(req).await.expect("không nâng cấp được WebSocket");
            let (mut tx, mut rx) = ws.split();
            tx.send(Message::text(setup_message(model, voice).to_string())).await.unwrap();

            let mut setup_ok = false;
            let mut audio_samples = 0usize;
            let mut model_text = String::new();
            let deadline = tokio::time::sleep(Duration::from_secs(40));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = &mut deadline => panic!("quá 40s chưa xong 1 lượt (setup={setup_ok}, audio={audio_samples})"),
                    msg = rx.next() => {
                        let msg = msg.expect("server đóng kết nối").expect("lỗi WebSocket");
                        if let Message::Close(f) = &msg { panic!("server đóng phiên: {f:?}"); }
                        let Some(text) = message_text(&msg) else { continue };
                        let ev = parse_server_message(&text);
                        if ev.setup_complete && !setup_ok {
                            setup_ok = true;
                            let ask = serde_json::json!({"realtimeInput": {"text": "Chào bạn, trả lời thật ngắn: 2 cộng 2 bằng mấy?"}});
                            tx.send(Message::text(ask.to_string())).await.unwrap();
                        }
                        audio_samples += ev.audio.iter().map(|(s, _)| s.len()).sum::<usize>();
                        if let Some(t) = ev.model_text { model_text.push_str(&t); }
                        if ev.turn_complete { break; }
                    }
                }
            }
            let _ = tx.send(Message::Close(None)).await;
            println!("Live: {:.1}s giọng AI, lời thoại: {model_text:?}", audio_samples as f32 / 24_000.0);
            assert!(setup_ok);
            assert!(audio_samples > 12_000, "AI gần như không nói gì");
        });
    }

    #[test]
    fn setup_has_audio_modality_and_transcription() {
        let v = setup_message("gemini-3.8-live", "Kore");
        assert_eq!(v["setup"]["generationConfig"]["speechConfig"]["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"], "Kore");
        assert_eq!(v["setup"]["generationConfig"]["responseModalities"][0], "AUDIO");
        assert!(v["setup"]["inputAudioTranscription"].is_object());
        assert!(v["setup"]["outputAudioTranscription"].is_object());
        assert!(v["setup"]["generationConfig"]["thinkingConfig"].is_null());
        let ext = setup_message("gemini-3.8-live-extended-thinking", "Kore");
        assert_eq!(ext["setup"]["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low");
    }
}
