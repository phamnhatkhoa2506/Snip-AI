//! "Snap Audio" — chế độ chụp thứ 3 bên cạnh ảnh/video: ghi âm MIC và/hoặc
//! ÂM THANH HỆ THỐNG (xem audio.rs) rồi gửi cho Gemini nghe (chép lời, tóm
//! tắt, dịch, trả lời câu hỏi trong đoạn ghi âm...). Cùng luồng với quay
//! video: 1 thanh công cụ nổi (đồng hồ + sóng âm + Dừng/Huỷ), ghi xong mới mở
//! cửa sổ "Kết quả AI" (hoặc nối vào chuỗi của cửa sổ đang mở — "+ Ghi thêm
//! audio"), audio nằm CHUNG chuỗi media với ảnh/video (`MediaKind::Audio`).
//!
//! Lưu WAV 16kHz mono: Gemini tự hạ mọi audio xuống ~16kbps trước khi nghe,
//! giữ chất lượng cao hơn chỉ làm file nặng vô ích — 2 phút chỉ ~3.8MB.
//!
//! Quyền: app desktop Win32 KHÔNG được Windows hỏi quyền micro bằng hộp
//! thoại — tắt quyền thì mở micro đơn giản là lỗi. Việc "xin phép" do chính
//! app làm (công tắc trong Cài đặt, mặc định TẮT — xem settings.ts), thanh
//! công cụ chỉ gọi `start_audio_snap` với đúng các nguồn đã được cho phép.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};

use crate::audio::{self, AudioSource, CaptureOptions};
use crate::commands::{self, AUDIO_LABEL_PREFIX};
use crate::state::{push_media, AppState, AudioSnapSession, MediaItem, MediaKind};

pub const AUDIO_TOOLBAR_LABEL: &str = "audio-toolbar";
const MAX_AUDIO_SECONDS: u64 = 120;
const SAMPLE_RATE: u32 = 16_000;
/// Ngắn hơn mức này coi như bấm nhầm — không mở cửa sổ kết quả.
const MIN_AUDIO_MS: u64 = 500;

/// Mở thanh công cụ Snap Audio — dùng chung cho nút "+ New" (chế độ Audio),
/// phím tắt, và "+ Ghi thêm audio" trong cửa sổ chat (`append_to`). Thanh
/// công cụ TỰ đọc cài đặt nguồn/quyền rồi gọi `start_audio_snap` — Rust
/// không đọc được localStorage nơi lưu các cài đặt đó.
pub fn open_audio_snap_toolbar(app: &AppHandle, append_to: Option<&str>) -> Result<(), String> {
    if !crate::oauth::is_logged_in() {
        commands::show_settings_window(app.clone());
        return Err("Cần đăng nhập Google trước khi dùng AI.".into());
    }
    let state = app.state::<AppState>();
    if state.audio_snap.lock().unwrap().is_some() {
        if let Some(win) = app.get_webview_window(AUDIO_TOOLBAR_LABEL) {
            let _ = win.set_focus();
        }
        return Ok(());
    }
    if let Some(win) = app.get_webview_window(AUDIO_TOOLBAR_LABEL) {
        let _ = win.close();
    }

    let url = match append_to {
        Some(label) => format!("audio-toolbar?appendTo={label}"),
        None => "audio-toolbar".to_string(),
    };
    let (mon_x, mon_y, mon_w, _mon_h, scale) = commands::primary_monitor_rect(app);
    let win_w = (320.0_f64 * scale).round();
    let win_h = (56.0_f64 * scale).round();
    let pos_x = mon_x as f64 + (mon_w as f64 - win_w) / 2.0;
    let pos_y = mon_y as f64 + (14.0 * scale).round();

    let win = WebviewWindowBuilder::new(app, AUDIO_TOOLBAR_LABEL, WebviewUrl::App(url.into()))
        .title("Đang ghi âm")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(true)
        .transparent(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được thanh ghi âm: {e}"))?;
    let _ = win.set_size(PhysicalSize::new(win_w, win_h));
    let _ = win.set_position(PhysicalPosition::new(pos_x, pos_y));
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

/// `async` — lệnh tạo cửa sổ, cùng lý do với `commands::trigger_capture`.
#[tauri::command]
pub async fn open_audio_snap(app: AppHandle, append_to: Option<String>) -> Result<(), String> {
    open_audio_snap_toolbar(&app, append_to.as_deref())
}

#[tauri::command]
pub async fn start_audio_snap(
    app: AppHandle,
    sources: Vec<String>,
    append_to: Option<String>,
    auto_copy: bool,
) -> Result<(), String> {
    let sources = AudioSource::parse_list(&sources);
    if sources.is_empty() {
        return Err("Chưa chọn nguồn âm thanh nào (micro hoặc âm thanh hệ thống).".into());
    }
    {
        let state = app.state::<AppState>();
        if state.audio_snap.lock().unwrap().is_some() {
            return Err("Đang có 1 phiên ghi âm khác chạy rồi.".into());
        }
    }

    let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_samples = samples.clone();
    let level_app = app.clone();
    let (handle, _) = tauri::async_runtime::spawn_blocking(move || {
        audio::start_capture(
            CaptureOptions { sources, sample_rate: SAMPLE_RATE, channels: 1, frame_ms: 100, strict: true },
            move |frame| {
                sink_samples.lock().unwrap().extend_from_slice(frame);
                let _ = level_app.emit_to(AUDIO_TOOLBAR_LABEL, "audio-snap:level", audio::level(frame));
            },
        )
    })
    .await
    .map_err(|e| format!("Lỗi nội bộ khi mở thiết bị âm thanh: {e}"))??;

    let stop = Arc::new(AtomicBool::new(false));
    let discard = Arc::new(AtomicBool::new(false));
    *app.state::<AppState>().audio_snap.lock().unwrap() =
        Some(AudioSnapSession { stop: stop.clone(), discard: discard.clone() });

    std::thread::spawn(move || {
        let started = Instant::now();
        while !stop.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(MAX_AUDIO_SECONDS) {
            std::thread::sleep(Duration::from_millis(100));
        }
        handle.stop();
        let samples = std::mem::take(&mut *samples.lock().unwrap());
        *app.state::<AppState>().audio_snap.lock().unwrap() = None;
        let discarded = discard.load(Ordering::SeqCst);

        // Tạo/đóng cửa sổ PHẢI trên main thread — xem giải thích ở lib.rs.
        let app_main = app.clone();
        let _ = app.run_on_main_thread(move || finish(&app_main, samples, discarded, append_to, auto_copy));
    });
    Ok(())
}

fn finish(app: &AppHandle, samples: Vec<i16>, discarded: bool, append_to: Option<String>, auto_copy: bool) {
    if let Some(win) = app.get_webview_window(AUDIO_TOOLBAR_LABEL) {
        let _ = win.close();
    }
    if discarded {
        return;
    }
    if (samples.len() as u64) * 1000 < SAMPLE_RATE as u64 * MIN_AUDIO_MS {
        eprintln!("[snip-ai][audio] Đoạn ghi âm quá ngắn, bỏ qua.");
        return;
    }

    let wav = audio::wav_from_pcm16(&samples, SAMPLE_RATE, 1);
    eprintln!("[snip-ai][audio] Ghi âm xong: {:.1}s, {} bytes WAV", samples.len() as f64 / SAMPLE_RATE as f64, wav.len());

    if auto_copy {
        // Không có định dạng clipboard chuẩn cho audio thô — chép kiểu FILE
        // (giống video, xem clipboard_copy.rs), cần ghi ra file tạm trước.
        let path = std::env::temp_dir().join(format!("snap-ai-audio-{}.wav", crate::record::uuid_like()));
        match std::fs::write(&path, &wav) {
            Ok(()) => crate::clipboard_copy::copy_file_to_clipboard(&path),
            Err(e) => eprintln!("[snip-ai][audio] Không ghi được file tạm để chép vào clipboard: {e}"),
        }
    }

    let state = app.state::<AppState>();
    // Cửa sổ đích có thể đã bị đóng trong lúc đang ghi âm — khi đó mở phiên
    // mới thay vì âm thầm vứt đoạn ghi âm đi.
    if let Some(label) = append_to.filter(|l| app.get_webview_window(l).is_some()) {
        push_media(&state, &label, MediaItem { bytes: wav, kind: MediaKind::Audio });
        let _ = app.emit_to(&label, "ai:chain-updated", ());
        return;
    }

    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    let label = format!("{AUDIO_LABEL_PREFIX}{session_id}");
    state.media_sessions.lock().unwrap().insert(label.clone(), vec![MediaItem { bytes: wav, kind: MediaKind::Audio }]);
    if let Err(e) = commands::open_centered_result_window(app, &label, "result") {
        eprintln!("[snip-ai][audio] {e}");
        state.media_sessions.lock().unwrap().remove(&label);
    }
}

#[tauri::command]
pub fn stop_audio_snap(state: State<'_, AppState>) -> Result<(), String> {
    match state.audio_snap.lock().unwrap().as_ref() {
        Some(session) => {
            session.stop.store(true, Ordering::SeqCst);
            Ok(())
        }
        None => Err("Không có phiên ghi âm nào đang chạy.".into()),
    }
}

/// Huỷ — không giữ đoạn ghi âm. Gọi được cả khi CHƯA bắt đầu thu (thanh
/// công cụ đang báo thiếu quyền) — khi đó chỉ đóng thanh công cụ.
#[tauri::command]
pub fn cancel_audio_snap(app: AppHandle) {
    let state = app.state::<AppState>();
    let guard = state.audio_snap.lock().unwrap();
    match guard.as_ref() {
        Some(session) => {
            session.discard.store(true, Ordering::SeqCst);
            session.stop.store(true, Ordering::SeqCst);
        }
        None => {
            if let Some(win) = app.get_webview_window(AUDIO_TOOLBAR_LABEL) {
                let _ = win.close();
            }
        }
    }
}

/// Thanh công cụ bị đóng bất thường (Alt+F4...) trong lúc đang thu — huỷ
/// phiên, không để micro bị giữ mở vô thời hạn. Gọi từ `on_window_event`.
pub fn on_toolbar_destroyed(app: &AppHandle) {
    let state = app.state::<AppState>();
    let guard = state.audio_snap.lock().unwrap();
    if let Some(session) = guard.as_ref() {
        session.discard.store(true, Ordering::SeqCst);
        session.stop.store(true, Ordering::SeqCst);
    }
}

/// Kiểm tra quyền micro ngay khi người dùng bật công tắc "Cho phép dùng
/// micro" trong Cài đặt — lỗi có tiền tố `MIC_PERMISSION_DENIED:` nếu Windows
/// đang chặn (frontend hiện nút mở trang cài đặt quyền riêng tư).
#[tauri::command]
pub async fn probe_microphone() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(audio::probe_microphone)
        .await
        .map_err(|e| format!("Lỗi nội bộ khi kiểm tra micro: {e}"))?
}

/// Mở thẳng trang "Quyền riêng tư > Micro" của Windows.
#[tauri::command]
pub fn open_mic_privacy_settings() -> Result<(), String> {
    // explorer.exe luôn trả mã thoát 1 kể cả khi mở thành công — chỉ cần
    // spawn được là đủ, không chờ/kiểm tra mã thoát.
    std::process::Command::new("explorer")
        .arg("ms-settings:privacy-microphone")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Không mở được Cài đặt Windows: {e}"))
}
