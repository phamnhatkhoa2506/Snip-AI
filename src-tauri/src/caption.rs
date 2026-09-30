//! PHỤ ĐỀ TRỰC TIẾP: cửa sổ nhỏ luôn nằm trên cùng, hiện chữ của âm thanh
//! đang phát trên máy (hoặc micro) theo thời gian thực — người dùng bật lên
//! rồi chuyển sang làm việc khác, vừa nghe vừa đọc chữ.
//!
//! Hai chế độ, cùng 1 đường ống thu âm -> WebSocket Live:
//!  - `transcribe` (gemini-3.5-transcribe-live): chép lại đúng lời nói.
//!  - `translate`  (gemini-3.5-live-translate-preview): dịch sang ngôn ngữ
//!    đích; chữ gốc hiện kèm bên dưới. Giọng dịch của model bị BỎ — phát ra
//!    loa thì âm thanh hệ thống thu lại chính nó (vòng lặp).
//!
//! Mỗi phiên Live tối đa 10 phút — tự nối lại trước hạn (và khi rớt mạng);
//! âm thanh trong lúc nối lại xếp hàng trong kênh, gửi bù ngay khi có kết nối.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::audio::{self, AudioSource, CaptureOptions};
use crate::history::HistoryTurn;
use crate::live::{audio_message, message_text, open_live_socket, parse_server_message, FRAME_MS, INPUT_RATE};
use crate::state::AppState;

pub const CAPTION_LABEL: &str = "caption";
const TRANSCRIBE_MODEL: &str = "gemini-3.5-transcribe-live";
const TRANSLATE_MODEL: &str = "gemini-3.5-live-translate-preview";
/// Nối lại trước mốc 10 phút của server để không bị cắt ngang câu.
const ROTATE_AFTER: Duration = Duration::from_secs(9 * 60 + 30);
/// Sau khi báo hết luồng âm thanh, chờ server chốt nốt câu cuối.
const DRAIN: Duration = Duration::from_secs(2);
const MAX_RECONNECT_FAILS: u32 = 4;
/// Ngôn ngữ đích cho phép — chặn mã lạ lọt vào cấu hình gửi lên Gemini.
const TARGET_LANGS: &[&str] = &[
    "vi", "en", "ja", "ko", "zh", "fr", "de", "es", "th", "id", "ru", "pt", "it", "hi", "ar",
];

pub enum CaptionCommand {
    Stop,
}

pub struct CaptionSession {
    commands: mpsc::UnboundedSender<CaptionCommand>,
    paused: Arc<AtomicBool>,
}

#[derive(Serialize, Clone)]
struct ChunkPayload {
    /// "interim" | "final" (chép lời) · "source" | "target" (dịch)
    channel: &'static str,
    text: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Transcribe,
    Translate,
}

fn setup_message(mode: Mode, target: &str) -> serde_json::Value {
    match mode {
        Mode::Transcribe => serde_json::json!({ "setup": {
            "model": format!("models/{TRANSCRIBE_MODEL}"),
            "generationConfig": { "responseModalities": ["TEXT"] },
            "inputAudioTranscription": { "languageCodes": [] },
            "realtimeInputConfig": { "automaticActivityDetection": { "disabled": false } }
        }}),
        Mode::Translate => serde_json::json!({ "setup": {
            "model": format!("models/{TRANSLATE_MODEL}"),
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "translationConfig": { "targetLanguageCode": target, "echoTargetLanguage": true }
            },
            "inputAudioTranscription": {},
            "outputAudioTranscription": {}
        }}),
    }
}

/// Gom chữ thành các dòng để lưu Lịch sử. Chép lời: mỗi câu chốt 1 dòng
/// `[mm:ss] câu`. Dịch: chữ gốc + bản dịch mỗi đoạn thành 1 cặp lượt.
struct CaptionLog {
    mode: Mode,
    started: Instant,
    turns: Vec<HistoryTurn>,
    src: String,
    dst: String,
}

impl CaptionLog {
    fn new(mode: Mode) -> Self {
        Self { mode, started: Instant::now(), turns: Vec::new(), src: String::new(), dst: String::new() }
    }

    fn final_line(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let t = self.started.elapsed().as_secs();
        self.turns.push(HistoryTurn {
            role: "assistant".into(),
            content: format!("[{:02}:{:02}] {text}", t / 60, t % 60),
            display_label: None,
        });
    }

    /// Đóng đoạn dịch hiện tại (hết câu / hết lượt).
    fn flush_pair(&mut self) {
        let (src, dst) = (self.src.trim().to_string(), self.dst.trim().to_string());
        self.src.clear();
        self.dst.clear();
        if !src.is_empty() {
            self.turns.push(HistoryTurn { role: "user".into(), content: src, display_label: None });
        }
        if !dst.is_empty() {
            self.turns.push(HistoryTurn { role: "assistant".into(), content: dst, display_label: None });
        }
    }

    fn into_turns(mut self) -> Vec<HistoryTurn> {
        if self.mode == Mode::Translate {
            self.flush_pair();
        }
        self.turns
    }
}

/// Đoạn dịch kết thúc ở dấu câu cuối câu và đã đủ dài -> ngắt dòng.
fn ends_sentence(s: &str) -> bool {
    s.trim_end().ends_with(['.', '!', '?', '。', '！', '？', '…'])
}

#[tauri::command]
pub async fn open_caption_window(app: AppHandle) -> Result<(), String> {
    if !crate::oauth::is_logged_in() {
        crate::commands::show_settings_window(app.clone());
        return Err("Cần đăng nhập Google trước khi dùng AI.".into());
    }
    if let Some(win) = app.get_webview_window(CAPTION_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let (mon_x, mon_y, mon_w, mon_h, scale) = crate::commands::primary_monitor_rect(&app);
    let win_w = (460.0_f64 * scale).round();
    let win_h = (300.0_f64 * scale).round();
    // Đáy màn hình, giữa — chỗ phụ đề quen thuộc, ít che nội dung đang xem.
    let pos_x = mon_x as f64 + (mon_w as f64 - win_w) / 2.0;
    let pos_y = mon_y as f64 + mon_h as f64 - win_h - 80.0 * scale;
    let win = WebviewWindowBuilder::new(&app, CAPTION_LABEL, WebviewUrl::App("caption".into()))
        .title("Phụ đề trực tiếp")
        .decorations(true)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được cửa sổ phụ đề: {e}"))?;
    let _ = win.set_size(PhysicalSize::new(win_w, win_h));
    let _ = win.set_min_size(Some(PhysicalSize::new(300.0 * scale, 160.0 * scale)));
    let _ = win.set_position(PhysicalPosition::new(pos_x.max(mon_x as f64), pos_y.max(mon_y as f64)));
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

#[tauri::command]
pub async fn caption_start(
    app: AppHandle,
    mode: String,
    sources: Vec<String>,
    target_lang: Option<String>,
) -> Result<(), String> {
    let mode = match mode.as_str() {
        "transcribe" => Mode::Transcribe,
        "translate" => Mode::Translate,
        _ => return Err("Chế độ phụ đề không hợp lệ.".into()),
    };
    let target = target_lang.filter(|l| TARGET_LANGS.contains(&l.as_str())).unwrap_or_else(|| "vi".into());
    let sources = AudioSource::parse_list(&sources);
    if sources.is_empty() {
        return Err("Chưa chọn nguồn âm thanh.".into());
    }
    if app.state::<AppState>().caption.lock().unwrap().is_some() {
        return Err("Đang có 1 phiên phụ đề chạy rồi.".into());
    }
    let model = if mode == Mode::Transcribe { TRANSCRIBE_MODEL } else { TRANSLATE_MODEL };

    // Kết nối TRƯỚC khi mở thu âm: lỗi mạng/đăng nhập báo ngay, chưa đụng micro.
    let first = open_live_socket(model, setup_message(mode, &target)).await?;

    let (frame_tx, mut frame_rx) = mpsc::unbounded_channel::<Vec<i16>>();
    let level_app = app.clone();
    let paused = Arc::new(AtomicBool::new(false));
    let paused_cap = paused.clone();
    let (capture, _) = tauri::async_runtime::spawn_blocking(move || {
        audio::start_capture(
            CaptureOptions { sources, sample_rate: INPUT_RATE, channels: 1, frame_ms: FRAME_MS, strict: true },
            move |frame| {
                let _ = level_app.emit_to(CAPTION_LABEL, "caption:level", audio::level(frame));
                if !paused_cap.load(Ordering::SeqCst) {
                    let _ = frame_tx.send(frame.to_vec());
                }
            },
        )
    })
    .await
    .map_err(|e| format!("Lỗi nội bộ khi mở nguồn âm thanh: {e}"))??;

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<CaptionCommand>();
    // Cửa sổ có thể đã đóng trong lúc đang kết nối — phiên sẽ tự dừng ngay.
    if app.get_webview_window(CAPTION_LABEL).is_none() {
        let _ = cmd_tx.send(CaptionCommand::Stop);
    }
    *app.state::<AppState>().caption.lock().unwrap() = Some(CaptionSession { commands: cmd_tx, paused });
    let _ = app.emit_to(CAPTION_LABEL, "caption:state", "listening");

    let loop_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let app = loop_app;
        let mut log = CaptionLog::new(mode);
        let started = Instant::now();
        let mut socket = Some(first);
        let mut fails = 0u32;

        let end_reason: String = 'session: loop {
            let (mut ws_tx, mut ws_rx) = match socket.take() {
                Some(s) => s,
                None => match open_live_socket(model, setup_message(mode, &target)).await {
                    Ok(s) => {
                        let _ = app.emit_to(CAPTION_LABEL, "caption:state", "listening");
                        s
                    }
                    Err(e) => {
                        fails += 1;
                        if fails >= MAX_RECONNECT_FAILS {
                            break format!("Không nối lại được: {e}");
                        }
                        // Lúc chờ, vẫn nhận lệnh dừng.
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(2 * fails as u64)) => {}
                            cmd = cmd_rx.recv() => if matches!(cmd, Some(CaptionCommand::Stop) | None) { break String::new() },
                        }
                        continue 'session;
                    }
                },
            };
            let conn_started = Instant::now();
            let rotate = tokio::time::sleep(ROTATE_AFTER);
            tokio::pin!(rotate);
            let mut got_text = false;

            // Kết quả của 1 kết nối: dừng hẳn / nối lại (kèm lý do nếu do lỗi).
            enum Outcome {
                Stop(String),
                Reconnect(Option<String>),
            }
            let outcome = loop {
                tokio::select! {
                    _ = &mut rotate => break Outcome::Reconnect(None),
                    cmd = cmd_rx.recv() => if matches!(cmd, Some(CaptionCommand::Stop) | None) { break Outcome::Stop(String::new()) },
                    frame = frame_rx.recv() => {
                        let Some(frame) = frame else { break Outcome::Stop("Nguồn âm thanh đã dừng.".into()) };
                        if ws_tx.send(Message::text(audio_message(&frame))).await.is_err() {
                            break Outcome::Reconnect(Some("Mất kết nối.".into()));
                        }
                    },
                    msg = ws_rx.next() => match msg {
                        Some(Ok(Message::Close(_))) | None => break Outcome::Reconnect(Some("Máy chủ đã đóng kết nối.".into())),
                        Some(Err(e)) => break Outcome::Reconnect(Some(format!("Mất kết nối: {e}"))),
                        Some(Ok(m)) => {
                            let Some(text) = message_text(&m) else { continue };
                            if handle_event(&app, &mut log, mode, &text) { got_text = true; fails = 0; }
                        }
                    },
                }
            };

            match outcome {
                Outcome::Stop(reason) => {
                    drain(&app, &mut log, mode, &mut ws_tx, &mut ws_rx).await;
                    let _ = ws_tx.send(Message::Close(None)).await;
                    break 'session reason;
                }
                Outcome::Reconnect(err) => {
                    // Nối lại theo lịch (không lỗi) thì báo hết luồng để server chốt câu cuối.
                    if err.is_none() {
                        drain(&app, &mut log, mode, &mut ws_tx, &mut ws_rx).await;
                    }
                    let _ = ws_tx.send(Message::Close(None)).await;
                    if err.is_some() && !got_text && conn_started.elapsed() < Duration::from_secs(5) {
                        fails += 1;
                    }
                    if fails >= MAX_RECONNECT_FAILS {
                        break 'session err.unwrap_or_else(|| "Mất kết nối.".into());
                    }
                    let _ = app.emit_to(CAPTION_LABEL, "caption:state", "reconnecting");
                }
            }
        };

        capture.stop();
        *app.state::<AppState>().caption.lock().unwrap() = None;
        crate::history::save_text_session(
            &app,
            "caption",
            if mode == Mode::Transcribe { TRANSCRIBE_MODEL } else { TRANSLATE_MODEL },
            log.into_turns(),
            started.elapsed().as_secs(),
        );
        let _ = app.emit_to(CAPTION_LABEL, "caption:ended", end_reason);
    });
    Ok(())
}

/// Báo hết luồng âm thanh rồi đọc nốt tin nhắn trong `DRAIN` để không mất câu cuối.
async fn drain<Tx, Rx>(app: &AppHandle, log: &mut CaptionLog, mode: Mode, ws_tx: &mut Tx, ws_rx: &mut Rx)
where
    Tx: futures_util::Sink<Message> + Unpin,
    Rx: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    if mode == Mode::Transcribe {
        let _ = ws_tx.send(Message::text(r#"{"realtimeInput":{"audioStreamEnd":true}}"#)).await;
    }
    let deadline = tokio::time::sleep(DRAIN);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            m = ws_rx.next() => match m {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(m)) => if let Some(t) = message_text(&m) { handle_event(app, log, mode, &t); },
            },
        }
    }
}

/// Xử lý 1 tin nhắn server: gửi chữ lên cửa sổ + ghi nhật ký. Trả về `true`
/// nếu có chữ mới.
fn handle_event(app: &AppHandle, log: &mut CaptionLog, mode: Mode, raw: &str) -> bool {
    let ev = parse_server_message(raw);
    let mut any = false;
    let emit = |channel: &'static str, text: String| {
        let _ = app.emit_to(CAPTION_LABEL, "caption:chunk", ChunkPayload { channel, text });
    };
    match mode {
        Mode::Transcribe => {
            if let Some(t) = ev.interim_input {
                emit("interim", t);
                any = true;
            }
            if let Some(t) = ev.user_text {
                log.final_line(&t);
                emit("final", t);
                any = true;
            }
        }
        Mode::Translate => {
            // `user_text` = chữ gốc đang nghe, `model_text` = bản dịch (từng mẩu).
            if let Some(t) = ev.user_text {
                log.src.push_str(&t);
                emit("source", t);
                any = true;
            }
            if let Some(t) = ev.model_text {
                log.dst.push_str(&t);
                let done = ends_sentence(&t) && log.dst.chars().count() > 60;
                emit("target", t);
                if done {
                    log.flush_pair();
                    let _ = app.emit_to(CAPTION_LABEL, "caption:break", ());
                }
                any = true;
            }
            if ev.turn_complete {
                log.flush_pair();
                let _ = app.emit_to(CAPTION_LABEL, "caption:break", ());
            }
        }
    }
    any
}

#[tauri::command]
pub fn caption_stop(state: State<'_, AppState>) {
    if let Some(s) = state.caption.lock().unwrap().as_ref() {
        let _ = s.commands.send(CaptionCommand::Stop);
    }
}

#[tauri::command]
pub fn caption_set_paused(state: State<'_, AppState>, paused: bool) {
    if let Some(s) = state.caption.lock().unwrap().as_ref() {
        s.paused.store(paused, Ordering::SeqCst);
    }
}

/// Cửa sổ phụ đề bị đóng — dừng phiên (nhả nguồn âm thanh, đóng kết nối).
pub fn on_window_destroyed(app: &AppHandle) {
    if let Some(s) = app.state::<AppState>().caption.lock().unwrap().as_ref() {
        let _ = s.commands.send(CaptionCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_messages_match_verified_protocol() {
        let t = setup_message(Mode::Transcribe, "vi");
        assert_eq!(t["setup"]["model"], "models/gemini-3.5-transcribe-live");
        assert_eq!(t["setup"]["generationConfig"]["responseModalities"][0], "TEXT");
        let x = setup_message(Mode::Translate, "en");
        assert_eq!(x["setup"]["generationConfig"]["translationConfig"]["targetLanguageCode"], "en");
    }

    #[test]
    fn log_pairs_translation_and_stamps_transcribe_lines() {
        let mut l = CaptionLog::new(Mode::Translate);
        l.src.push_str(" Xin chào ");
        l.dst.push_str(" Hello ");
        l.flush_pair();
        let turns = l.into_turns();
        assert_eq!((turns[0].role.as_str(), turns[0].content.as_str()), ("user", "Xin chào"));
        assert_eq!((turns[1].role.as_str(), turns[1].content.as_str()), ("assistant", "Hello"));

        let mut t = CaptionLog::new(Mode::Transcribe);
        t.final_line("  ");
        t.final_line("Chào bạn");
        let turns = t.into_turns();
        assert_eq!(turns.len(), 1);
        assert!(turns[0].content.starts_with("[00:00] Chào bạn"));
    }

    #[test]
    fn sentence_end_detection() {
        assert!(ends_sentence("Hello world. "));
        assert!(ends_sentence("你好。"));
        assert!(!ends_sentence("Hello wor"));
    }
}
