//! Phím tắt có thể tuỳ chỉnh — dùng cho CẢ 2 mục đích: chụp màn hình (mặc
//! định `Ctrl+PrintScreen`) VÀ quay video màn hình (mặc định
//! `Ctrl+Shift+PrintScreen`, xem record.rs). 2 phím tắt độc lập hoàn toàn —
//! đổi 1 cái không ảnh hưởng cái kia.
//!
//! - Lưu dưới dạng chuỗi "accelerator" (VD "Ctrl+PrintScreen", "Ctrl+Alt+S")
//!   vào 1 file text trong thư mục config của app — không cần database.
//! - `set_hotkey`/`set_record_hotkey` unregister phím cũ + register phím
//!   mới; nếu phím mới đăng ký thất bại (VD đã bị app khác/Windows chiếm),
//!   TỰ ĐỘNG rollback lại phím cũ để không bao giờ mất hẳn tính năng.
//!
//! Các hàm dùng chung (`load_saved_accelerator`, `register_initial`...) nhận
//! `file_name`/`default` làm tham số thay vì hardcode, để tái dùng được cho
//! cả 2 loại phím tắt mà không phải chép code 2 lần.

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::{HotkeyState, RecordHotkeyState};

pub const DEFAULT_ACCELERATOR: &str = "Ctrl+PrintScreen";
pub const CONFIG_FILE_NAME: &str = "hotkey.txt";

/// Mặc định lệch hẳn với phím snip ảnh (thêm Shift) — cùng "họ" PrintScreen
/// cho dễ nhớ, nhưng không trùng để bấm nhầm giữa 2 tính năng.
pub const DEFAULT_RECORD_ACCELERATOR: &str = "Ctrl+Shift+PrintScreen";
pub const RECORD_CONFIG_FILE_NAME: &str = "record-hotkey.txt";

fn config_file_path(app: &AppHandle, file_name: &str) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(file_name))
}

/// Đọc accelerator đã lưu trên đĩa; trả về mặc định nếu chưa có file/lỗi đọc.
pub fn load_saved_accelerator(app: &AppHandle, file_name: &str, default: &str) -> String {
    config_file_path(app, file_name)
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn save_accelerator(app: &AppHandle, file_name: &str, accel: &str) -> Result<(), String> {
    let path = config_file_path(app, file_name).ok_or("Không xác định được thư mục cấu hình")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Không tạo được thư mục cấu hình: {e}"))?;
    }
    fs::write(&path, accel).map_err(|e| format!("Không ghi được file cấu hình: {e}"))
}

/// Parse accelerator, dùng cho cả lúc khởi động và lúc người dùng đổi phím tắt.
/// Trả lỗi dạng tiếng Việt dễ hiểu thay vì lỗi gốc của thư viện.
fn parse_accelerator(accel: &str) -> Result<Shortcut, String> {
    Shortcut::from_str(accel).map_err(|e| format!("Không hiểu tổ hợp phím \"{accel}\": {e}"))
}

/// Parse accelerator đã lưu; nếu lỗi (VD file bị sửa tay sai định dạng) thì
/// rơi về mặc định thay vì crash app lúc khởi động.
pub fn resolve_initial_shortcut(app: &AppHandle, file_name: &str, default: &str) -> (Shortcut, String) {
    let saved = load_saved_accelerator(app, file_name, default);
    match parse_accelerator(&saved) {
        Ok(shortcut) => (shortcut, saved),
        Err(err) => {
            eprintln!("[snip-ai] Phím tắt đã lưu ({file_name}) không hợp lệ ({err}), dùng mặc định.");
            let default_shortcut = parse_accelerator(default)
                .expect("Giá trị mặc định phải luôn parse được — lỗi code, không phải lỗi người dùng");
            (default_shortcut, default.to_string())
        }
    }
}

/// Thử ĐĂNG KÝ phím tắt lúc khởi động (khác với `resolve_initial_shortcut` —
/// hàm đó chỉ lo phần PARSE, không đụng tới việc đăng ký thật với OS).
///
/// QUAN TRỌNG — bug đã gặp thực tế: combo đã lưu (hoặc mặc định
/// "Ctrl+PrintScreen") có thể parse hợp lệ nhưng ĐĂNG KÝ THẤT BẠI, vì lý do
/// nằm ngoài tầm kiểm soát của app — Windows/app khác đã chiếm mất tổ hợp
/// đó, hoặc (đặc biệt hay gặp với PrintScreen) driver bàn phím/tính năng
/// "Snip & Sketch" tích hợp sẵn của Windows chặn mất phím trước khi app kịp
/// nhận. TRƯỚC ĐÂY lỗi này làm CẢ APP CRASH NGAY LÚC MỞ (code cũ dùng `?`
/// trong `setup()`, khiến lỗi đăng ký propagate thành lỗi khởi động app) —
/// người dùng tưởng "phím tắt không dùng được" nhưng thực ra app còn chưa
/// kịp mở. Hàm này KHÔNG BAO GIỜ để lỗi đăng ký làm crash app: thử lần lượt
/// combo đã lưu -> combo mặc định (nếu khác) -> chịu thua và để app chạy mà
/// KHÔNG có phím tắt nào cả (vẫn mở được qua tray icon để tự chọn combo khác).
///
/// Trả về `(shortcut, accelerator, đã_đăng_ký_thành_công)`.
pub fn register_initial(app: &AppHandle, file_name: &str, default: &str) -> (Shortcut, String, bool) {
    let gs = app.global_shortcut();
    let (shortcut, accel) = resolve_initial_shortcut(app, file_name, default);

    if gs.register(shortcut).is_ok() {
        return (shortcut, accel, true);
    }
    eprintln!("[snip-ai] Không đăng ký được phím tắt đã lưu \"{accel}\" — có thể đã bị Windows hoặc app khác chiếm.");

    if accel != default {
        let default_shortcut = parse_accelerator(default)
            .expect("Giá trị mặc định phải luôn parse được — lỗi code, không phải lỗi người dùng");
        if gs.register(default_shortcut).is_ok() {
            eprintln!("[snip-ai] Đã dùng phím tắt mặc định thay thế: {default}");
            return (default_shortcut, default.to_string(), true);
        }
        eprintln!("[snip-ai] Phím tắt mặc định cũng không đăng ký được trên máy này.");
    }

    eprintln!("[snip-ai] Chưa có phím tắt nào hoạt động cho \"{file_name}\" — mở Cài đặt qua icon khay hệ thống để tự chọn tổ hợp khác.");
    (shortcut, accel, false)
}

/// Đổi phím tắt — logic dùng chung cho cả `set_hotkey` và `set_record_hotkey`.
fn set_shortcut(app: &AppHandle, current: &Mutex<Shortcut>, file_name: &str, accelerator: &str) -> Result<String, String> {
    let accelerator = accelerator.trim();
    if accelerator.is_empty() {
        return Err("Tổ hợp phím rỗng".into());
    }
    let new_shortcut = parse_accelerator(accelerator)?;

    let gs = app.global_shortcut();
    let mut guard = current.lock().unwrap();
    let old_shortcut = *guard;

    if old_shortcut == new_shortcut {
        return Ok(new_shortcut.to_string()); // không đổi gì, khỏi unregister/register lại
    }

    let _ = gs.unregister(old_shortcut);
    if let Err(e) = gs.register(new_shortcut) {
        // Rollback ngay để không bao giờ mất hẳn khả năng dùng tính năng chỉ
        // vì 1 lần đổi phím tắt thất bại (VD phím đó đã bị app khác/Windows chiếm).
        let _ = gs.register(old_shortcut);
        // Lỗi này KHÔNG phải bug của app — Windows chỉ cho phép 1 app giữ mỗi
        // tổ hợp phím tắt toàn cục tại 1 thời điểm, và một số tổ hợp còn bị
        // chính Windows/driver bàn phím giữ sẵn cho tính năng khác (VD
        // PrintScreen thường bị Windows dùng cho "Snip & Sketch" tích hợp sẵn,
        // Ctrl+Alt+... hay bị phần mềm gõ tiếng Việt/diệt virus/game giữ). Nếu
        // gặp lỗi này, không có cách nào "ép" đăng ký được từ phía app — chỉ
        // có thể thử tổ hợp khác.
        return Err(format!(
            "Không đăng ký được tổ hợp phím này — có thể đang bị Windows, driver bàn phím, hoặc app khác (bộ gõ, diệt virus, game...) giữ sẵn cho việc khác. Hãy thử 1 tổ hợp khác. Chi tiết kỹ thuật: {e}"
        ));
    }

    *guard = new_shortcut;
    let result_str = new_shortcut.to_string();
    drop(guard);

    save_accelerator(app, file_name, accelerator)?;
    Ok(result_str)
}

#[tauri::command]
pub fn get_hotkey(state: State<'_, HotkeyState>) -> String {
    state.current.lock().unwrap().to_string()
}

#[tauri::command]
pub fn set_hotkey(app: AppHandle, state: State<'_, HotkeyState>, accelerator: String) -> Result<String, String> {
    set_shortcut(&app, &state.current, CONFIG_FILE_NAME, &accelerator)
}

#[tauri::command]
pub fn get_record_hotkey(state: State<'_, RecordHotkeyState>) -> String {
    state.current.lock().unwrap().to_string()
}

#[tauri::command]
pub fn set_record_hotkey(app: AppHandle, state: State<'_, RecordHotkeyState>, accelerator: String) -> Result<String, String> {
    set_shortcut(&app, &state.current, RECORD_CONFIG_FILE_NAME, &accelerator)
}
