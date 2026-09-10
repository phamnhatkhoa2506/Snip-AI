mod ai;
mod capture;
mod commands;
mod hotkey;
mod oauth;
mod record;
mod secrets;
mod state;

use std::sync::Mutex;
use state::{AppState, HotkeyState, HttpClientState, RecordHotkeyState};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::ShortcutState;

pub const MAIN_LABEL: &str = "main";

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // BẮT BUỘC đăng ký ĐẦU TIÊN (khuyến nghị chính thức của Tauri): nếu
        // chạy lại .exe trong khi app đã có 1 instance đang chạy nền, instance
        // MỚI sẽ bị plugin này chặn ngay lập tức (không chạy tiếp setup()/tạo
        // cửa sổ) và forward lệnh gọi này cho instance CŨ xử lý — instance cũ
        // sẽ show + focus cửa sổ Settings, giống hệt cách Snipping Tool xử lý
        // khi mở lại trong lúc đã chạy nền.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(AppState::default())
        .manage(HttpClientState::default())
        .on_window_event(|window, event| {
            if window.label() == MAIN_LABEL {
                // Đóng cửa sổ Settings ("main") KHÔNG được làm thoát cả app —
                // app này chạy nền qua global hotkey + system tray. Đóng chỉ
                // ẩn đi, mở lại qua tray icon (menu "Mở Cài đặt" / click trái).
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            } else if window.label().starts_with(commands::RESULT_LABEL_PREFIX)
                || window.label().starts_with(commands::RECORD_LABEL_PREFIX)
            {
                // Cửa sổ "Kết quả AI" đóng thật (không ẩn) — dọn ảnh/video của
                // phiên đó khỏi bộ nhớ (crop_sessions/video_sessions), tránh rò
                // rỉ khi snip/quay nhiều lần trong 1 phiên làm việc dài. Thử xoá
                // ở CẢ HAI map luôn cho đơn giản — label chỉ khớp đúng 1 trong 2,
                // xoá key không tồn tại ở map còn lại là no-op, không lỗi gì.
                if let WindowEvent::Destroyed = event {
                    let state = window.state::<AppState>();
                    state.crop_sessions.lock().unwrap().remove(window.label());
                    state.video_sessions.lock().unwrap().remove(window.label());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::trigger_capture,
            commands::get_screenshot_base64,
            commands::cancel_overlay,
            commands::crop_and_open_result,
            commands::get_crop_image_base64,
            ai::ask_ai_nvidia,
            ai::ask_ai_openai,
            ai::ask_ai_anthropic,
            ai::ask_ai_gemini,
            secrets::save_api_key,
            secrets::delete_api_key,
            secrets::api_key_statuses,
            hotkey::get_hotkey,
            hotkey::set_hotkey,
            hotkey::get_record_hotkey,
            hotkey::set_record_hotkey,
            commands::trigger_recording_from_ui,
            commands::start_region_recording,
            commands::cancel_recording,
            oauth::start_google_login,
            oauth::get_login_status,
            oauth::logout,
            record::stop_recording,
            record::get_recording_base64,
        ])
        .setup(|app| {
            // Phím tắt giờ tuỳ chỉnh được (đọc từ file cấu hình đã lưu, mặc
            // định Ctrl+PrintScreen — cố tình lệch với PrintScreen mặc định
            // của Windows Snipping Tool). Handler LUÔN đọc phím tắt hiện tại
            // từ `HotkeyState` (không capture giá trị cố định vào closure) để
            // người dùng đổi phím tắt trong lúc app đang chạy vẫn hoạt động
            // đúng ngay, không cần khởi động lại app.
            //
            // Việc ĐĂNG KÝ thật với OS chỉ làm được SAU KHI plugin global-shortcut
            // cài xong (app.global_shortcut() cần plugin đã có) — nên tạm quản lý
            // HotkeyState với 1 giá trị "dự kiến" trước, rồi cập nhật lại đúng giá
            // trị THẬT SỰ đã đăng ký được (có thể khác, nếu combo đã lưu thất bại
            // và phải rơi về mặc định) ngay sau khi cài plugin xong.
            let (initial_shortcut, _initial_accel) =
                hotkey::resolve_initial_shortcut(&app.handle(), hotkey::CONFIG_FILE_NAME, hotkey::DEFAULT_ACCELERATOR);
            app.manage(HotkeyState {
                current: Mutex::new(initial_shortcut),
            });

            // Phím tắt QUAY VIDEO — độc lập hoàn toàn với phím snip ảnh ở trên
            // (xem giải thích chi tiết ở hotkey.rs/record.rs).
            let (initial_record_shortcut, _initial_record_accel) = hotkey::resolve_initial_shortcut(
                &app.handle(),
                hotkey::RECORD_CONFIG_FILE_NAME,
                hotkey::DEFAULT_RECORD_ACCELERATOR,
            );
            app.manage(RecordHotkeyState {
                current: Mutex::new(initial_record_shortcut),
            });

            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, shortcut, event| {
                        if event.state() != ShortcutState::Pressed {
                            return;
                        }
                        // 1 handler DÙNG CHUNG nhận sự kiện cho MỌI shortcut đã
                        // đăng ký (không phải 1 handler riêng/shortcut) — so
                        // sánh với CẢ HAI phím tắt hiện tại (snip ảnh + quay
                        // video) để biết bấm cái nào.
                        let snip_hotkey = *app.state::<HotkeyState>().current.lock().unwrap();
                        let record_hotkey = *app.state::<RecordHotkeyState>().current.lock().unwrap();

                        // QUAN TRỌNG: handler này chạy trên thread riêng của
                        // global-shortcut, KHÔNG phải main thread. Tạo cửa sổ
                        // WebView2 từ 1 thread không phải main thread có thể
                        // crash tuỳ điều kiện (đã gặp thực tế: process chết đột
                        // ngột ngay sau khi bấm hotkey). Ép chạy đúng trên main
                        // thread bằng `run_on_main_thread` cho cả 2 trường hợp.
                        if *shortcut == snip_hotkey {
                            let app_clone = app.clone();
                            let _ = app.run_on_main_thread(move || {
                                if let Err(err) = commands::capture_and_open_overlay(&app_clone, "overlay") {
                                    eprintln!("[snip-ai] Lỗi khi chụp màn hình: {err}");
                                }
                            });
                        } else if *shortcut == record_hotkey {
                            let app_clone = app.clone();
                            let _ = app.run_on_main_thread(move || {
                                if let Err(err) = commands::trigger_recording(&app_clone) {
                                    eprintln!("[snip-ai] Lỗi khi bắt đầu quay video: {err}");
                                }
                            });
                        }
                    })
                    .build(),
            )?;

            // Thử đăng ký thật — KHÔNG dùng `?` ở đây (khác code cũ): nếu combo
            // đã lưu/mặc định bị Windows hoặc app khác chiếm mất, app vẫn phải
            // MỞ ĐƯỢC bình thường (chỉ mất tính năng hotkey, sửa được qua Cài
            // đặt) thay vì crash ngay lúc khởi động — bug thực tế đã gặp, xem
            // giải thích chi tiết ở `hotkey::register_initial`.
            let (registered_shortcut, registered_accel, ok) =
                hotkey::register_initial(&app.handle(), hotkey::CONFIG_FILE_NAME, hotkey::DEFAULT_ACCELERATOR);
            *app.state::<HotkeyState>().current.lock().unwrap() = registered_shortcut;
            if ok {
                eprintln!("[snip-ai] Đã đăng ký phím tắt chụp màn hình: {registered_accel}");
            }

            let (registered_record_shortcut, registered_record_accel, record_ok) = hotkey::register_initial(
                &app.handle(),
                hotkey::RECORD_CONFIG_FILE_NAME,
                hotkey::DEFAULT_RECORD_ACCELERATOR,
            );
            *app.state::<RecordHotkeyState>().current.lock().unwrap() = registered_record_shortcut;
            if record_ok {
                eprintln!("[snip-ai] Đã đăng ký phím tắt quay video: {registered_record_accel}");
            }

            // ── System tray ──────────────────────────────────────────────
            let show_item = MenuItem::with_id(app, "show", "Mở Cài đặt", true, None::<&str>)?;
            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let autostart_item = CheckMenuItem::with_id(
                app,
                "autostart",
                "Khởi động cùng Windows",
                true,
                autostart_enabled,
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "Thoát", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show_item,
                    &autostart_item,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;

            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Snap AI — chạy nền, chờ phím tắt chụp màn hình")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "autostart" => {
                        let mgr = app.autolaunch();
                        let enabled = mgr.is_enabled().unwrap_or(false);
                        let result = if enabled { mgr.disable() } else { mgr.enable() };
                        if let Err(e) = result {
                            eprintln!("[snip-ai] Lỗi bật/tắt autostart: {e}");
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
