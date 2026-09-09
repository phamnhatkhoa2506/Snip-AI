use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};

use crate::capture;
use crate::state::{AppState, MonitorBounds};

const OVERLAY_LABEL: &str = "overlay";
/// Tiền tố label cho MỌI cửa sổ "Kết quả AI" — mỗi lần snip tạo 1 label MỚI
/// (VD "result-3"), không dùng chung 1 label cố định như trước nữa. Nhờ vậy
/// snip nhiều lần liên tiếp mà không đóng popup cũ sẽ mở NHIỀU cửa sổ độc
/// lập, thay vì cửa sổ mới đóng mất cửa sổ cũ (bug đã gặp thực tế).
pub const RESULT_LABEL_PREFIX: &str = "result-";

// Lưu ý đơn vị: TOÀN BỘ toạ độ/kích thước trong file này là PHYSICAL pixel
// (khớp trực tiếp pixel ảnh chụp từ `xcap`), KHÔNG phải logical pixel của
// Tauri. Cửa sổ luôn được tạo ẩn rồi set_position/set_size bằng kiểu Physical*
// ngay sau đó, thay vì dùng builder.position()/.inner_size() (vốn nhận logical
// pixel) — tránh lệch vị trí/kích thước trên màn hình có DPI scale != 100%.

/// Chụp toàn màn hình (primary monitor) rồi mở/hiện cửa sổ overlay fullscreen
/// để người dùng kéo chọn vùng. Gọi khi hotkey được bấm.
///
/// Overlay vẫn là SINGLETON (chỉ 1 cửa sổ chọn-vùng tại 1 thời điểm) — hợp lý
/// vì 1 chuột không kéo-chọn được 2 vùng cùng lúc. Khác với cửa sổ "Kết quả
/// AI" bên dưới, cái đó mới cần multi-instance.
pub fn capture_and_open_overlay(app: &AppHandle) -> Result<(), String> {
    let t0 = std::time::Instant::now();

    // Ẩn cửa sổ Settings ("main") trước khi chụp — nếu đang mở, nó sẽ che mất
    // vùng màn hình người dùng muốn chọn/nằm lẫn trong ảnh chụp. Chỉ `hide()`
    // (không close) để giữ nguyên state React/Svelte bên trong, mở lại được
    // ngay qua tray icon như bình thường.
    // Ẩn TẠM cửa sổ Settings trong lúc chọn vùng (nếu đang mở) — chỉ để nhường
    // chỗ, KHÔNG phải trạng thái cố định: sẽ tự hiện lại ngay khi chọn xong
    // hoặc bấm huỷ (xem `restore_main_after_snip` gọi ở cuối `crop_and_open_result`
    // và trong `cancel_overlay`).
    let mut hid_main = false;
    if let Some(main_win) = app.get_webview_window(crate::MAIN_LABEL) {
        if main_win.is_visible().unwrap_or(false) {
            let _ = main_win.hide();
            hid_main = true;
        }
    }
    *app.state::<AppState>().main_hidden_for_snip.lock().unwrap() = hid_main;
    if hid_main {
        // Chờ 1 nhịp ngắn để Windows xử lý xong việc ẩn cửa sổ trước khi chụp —
        // tránh trường hợp hiếm gặp cửa sổ vẫn còn kịp dính vào ảnh chụp do
        // hide() là thao tác bất đồng bộ ở tầng OS.
        std::thread::sleep(std::time::Duration::from_millis(60));
    }

    let (png, x, y, width, height) = capture::capture_primary_monitor()?;
    eprintln!(
        "[snip-ai][timing] chụp+encode PNG full màn hình: {:.0}ms ({} bytes)",
        t0.elapsed().as_secs_f64() * 1000.0,
        png.len()
    );

    let scale_factor = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);

    let state = app.state::<AppState>();
    *state.screenshot_png.lock().unwrap() = Some(png);
    *state.monitor_bounds.lock().unwrap() = Some(MonitorBounds { x, y, width, height });
    *state.scale_factor.lock().unwrap() = scale_factor;

    // Luôn ĐÓNG HẲN cửa sổ cũ (nếu có) rồi tạo mới, thay vì ẩn/hiện để tái dùng.
    // Đã gặp thực tế: ẩn/hiện lặp lại nhiều lần có thể làm hỏng kênh IPC của
    // WebView2 (native error "PostMessage failed... Invalid window handle"),
    // khiến mọi invoke()/fetch() sau đó bị treo vô thời hạn không lỗi không gì.
    // Tốn thêm ~1s tạo cửa sổ mỗi lần, đổi lại tránh hẳn lớp bug rất khó debug này.
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.close();
    }
    let win = WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("overlay".into()))
        .title("Snip-AI Overlay")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .transparent(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được cửa sổ overlay: {e}"))?;
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let _ = win.set_size(PhysicalSize::new(width, height));
    let _ = win.show();
    let _ = win.set_focus();

    eprintln!(
        "[snip-ai][timing] TỔNG capture_and_open_overlay (chụp + tạo/hiện overlay): {:.0}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

/// Cho phép kích hoạt luồng chụp thủ công từ UI (VD nút "Test chụp" trong cửa
/// sổ Settings), không chỉ qua global hotkey — tiện để debug khi hotkey chưa
/// đăng ký được trên máy nào đó.
///
/// `async fn`: BẮT BUỘC — lệnh này gọi được từ IPC (invoke() ở frontend), và
/// nó tạo cửa sổ mới (`WebviewWindowBuilder::build()`). Trên Windows/WebView2,
/// gọi tạo cửa sổ TRỰC TIẾP trong 1 command đồng bộ (sync fn) được dispatch từ
/// luồng IPC có thể tự-deadlock (lệnh chờ main thread xử lý việc tạo cửa sổ,
/// trong khi chính main thread đang bị command này chiếm dụng). Đánh dấu
/// `async` để Tauri chạy command trên async runtime (tách khỏi luồng IPC/main
/// thread), tránh deadlock — đây là khuyến nghị chính thức của Tauri cho các
/// command thao tác cửa sổ.
#[tauri::command]
pub async fn trigger_capture(app: AppHandle) -> Result<(), String> {
    capture_and_open_overlay(&app)
}

#[tauri::command]
pub fn get_screenshot_base64(state: State<'_, AppState>) -> Result<String, String> {
    let guard = state.screenshot_png.lock().unwrap();
    let bytes = guard.as_ref().ok_or("Chưa có ảnh chụp màn hình nào")?;
    Ok(STANDARD.encode(bytes))
}

#[tauri::command]
pub async fn cancel_overlay(app: AppHandle) {
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.close();
    }
    restore_main_after_snip(&app);
}

/// Hiện lại cửa sổ Settings ("main") nếu nó vừa bị `capture_and_open_overlay`
/// ẩn tạm để nhường chỗ cho việc chọn vùng — gọi ở CUỐI luồng chọn vùng, dù kết
/// quả là chọn xong (`crop_and_open_result`) hay huỷ (`cancel_overlay`). Không
/// làm gì nếu Settings vốn đã đóng/ẩn từ trước lúc bấm hotkey.
fn restore_main_after_snip(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut hid_main = state.main_hidden_for_snip.lock().unwrap();
    if *hid_main {
        if let Some(main_win) = app.get_webview_window(crate::MAIN_LABEL) {
            let _ = main_win.show();
        }
        *hid_main = false;
    }
}

/// Người dùng đã kéo chọn xong 1 vùng trong overlay. `x,y,width,height` PHẢI
/// là physical pixel tương đối trong ảnh chụp — frontend tự quy đổi từ CSS px
/// sang physical px bằng `window.devicePixelRatio` trước khi gọi (xem
/// overlay/+page.svelte).
///
/// Mỗi lần gọi lệnh này tạo 1 PHIÊN MỚI hoàn toàn độc lập (label cửa sổ riêng,
/// ảnh lưu riêng trong `crop_sessions`) — snip nhiều lần liên tiếp mà không
/// đóng popup cũ sẽ có nhiều cửa sổ "Kết quả AI" cùng tồn tại, không cái nào
/// đóng cái nào.
///
/// `async fn` — xem giải thích chi tiết ở `trigger_capture` phía trên: lệnh
/// này tạo cửa sổ mới (result window), nếu để sync fn sẽ bị treo (deadlock)
/// khi gọi qua IPC từ frontend — đây chính là bug đã xảy ra thực tế.
#[tauri::command]
pub async fn crop_and_open_result(
    app: AppHandle,
    state: State<'_, AppState>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    eprintln!("[snip-ai] crop_and_open_result: nhận x={x} y={y} w={width} h={height}");

    let screenshot = {
        let guard = state.screenshot_png.lock().unwrap();
        guard.clone().ok_or("Chưa có ảnh chụp màn hình nào")?
    };
    let monitor = {
        let guard = state.monitor_bounds.lock().unwrap();
        guard.ok_or("Chưa có thông tin màn hình")?
    };
    eprintln!("[snip-ai] monitor bounds: {monitor:?}, screenshot {} bytes", screenshot.len());

    let cropped = capture::crop_png(&screenshot, x, y, width, height)?;
    eprintln!("[snip-ai] crop xong, {} bytes PNG", cropped.len());

    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    let window_label = format!("{RESULT_LABEL_PREFIX}{session_id}");
    state.crop_sessions.lock().unwrap().insert(window_label.clone(), cropped);

    let crop_bounds_x = monitor.x + x as i32;
    let crop_bounds_y = monitor.y + y as i32;

    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.close();
    }
    eprintln!("[snip-ai] đã đóng overlay, chuẩn bị tạo cửa sổ kết quả (label={window_label})");

    open_result_window(&app, monitor, &window_label, crop_bounds_x, crop_bounds_y, height, session_id)?;
    // Chọn vùng xong, hiện lại Settings nếu vừa bị ẩn tạm — gọi SAU khi cửa sổ
    // kết quả đã show()+focus() để cửa sổ kết quả vẫn nằm trên cùng.
    restore_main_after_snip(&app);
    eprintln!(
        "[snip-ai][timing] TỔNG crop_and_open_result (crop + tạo/hiện cửa sổ kết quả): {:.0}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn open_result_window(
    app: &AppHandle,
    monitor: MonitorBounds,
    window_label: &str,
    crop_x: i32,
    crop_y: i32,
    crop_height: u32,
    session_id: u32,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let scale = *state.scale_factor.lock().unwrap();
    let scale = if scale > 0.0 { scale } else { 1.0 };

    // Kích thước UI mong muốn tính theo "logical px" cho quen mắt khi thiết kế,
    // rồi nhân với scale factor để ra physical px thật sự set cho window.
    let win_w = (480.0_f64 * scale).round();
    let win_h = (340.0_f64 * scale).round();
    let gap = (8.0_f64 * scale).round();

    // Lưu ý: KHÔNG dùng `.clamp(min, max)` trực tiếp — nó panic nếu min > max,
    // có thể xảy ra khi win_w > monitor.width (màn hình rất nhỏ / scale cao).
    // Dùng min/max lồng nhau thay thế, không bao giờ panic.
    let min_x = monitor.x as f64;
    let max_x = (monitor.x + monitor.width as i32) as f64 - win_w;
    let mut pos_x = (crop_x as f64).max(min_x).min(max_x.max(min_x));

    let mut pos_y = (crop_y + crop_height as i32) as f64 + gap;
    if pos_y + win_h > (monitor.y + monitor.height as i32) as f64 {
        pos_y = (crop_y as f64 - win_h - gap).max(monitor.y as f64);
    }

    // Lệch nhẹ (cascade) theo session_id để nhiều cửa sổ mở cùng 1 khu vực
    // màn hình không chồng khít lên nhau — dễ nhận ra có nhiều popup hơn.
    let cascade = (session_id % 6) as f64 * (18.0 * scale);
    pos_x = (pos_x + cascade).min(max_x.max(min_x));
    pos_y = (pos_y + cascade).min((monitor.y + monitor.height as i32) as f64 - win_h);

    eprintln!(
        "[snip-ai] open_result_window({window_label}): pos=({pos_x},{pos_y}) size=({win_w},{win_h}) scale={scale}"
    );

    // KHÔNG còn đóng cửa sổ result nào khác — mỗi phiên có label riêng nên
    // luôn tạo mới, không đụng tới các cửa sổ "Kết quả AI" khác đang mở.
    let win = WebviewWindowBuilder::new(app, window_label, WebviewUrl::App("result".into()))
        .title("Kết quả AI")
        .decorations(true)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được cửa sổ kết quả: {e}"))?;
    let _ = win.set_size(PhysicalSize::new(win_w, win_h));
    let _ = win.set_min_size(Some(PhysicalSize::new(360.0 * scale, 280.0 * scale)));
    let _ = win.set_position(PhysicalPosition::new(pos_x, pos_y));
    let _ = win.show();
    let _ = win.set_focus();
    eprintln!("[snip-ai] result window ({window_label}) đã show");

    Ok(())
}

/// `window_label`: label của cửa sổ "Kết quả AI" đang gọi lệnh này (frontend
/// tự đọc qua `getCurrentWindow().label` rồi truyền vào) — xác định đúng ảnh
/// của PHIÊN đó, vì giờ nhiều cửa sổ có thể mở cùng lúc, mỗi cửa sổ 1 ảnh
/// khác nhau.
#[tauri::command]
pub fn get_crop_image_base64(state: State<'_, AppState>, window_label: String) -> Result<String, String> {
    let sessions = state.crop_sessions.lock().unwrap();
    let bytes = sessions
        .get(&window_label)
        .ok_or("Không tìm thấy ảnh cho phiên này (cửa sổ có thể đã bị đóng/dọn dẹp)")?;
    Ok(STANDARD.encode(bytes))
}
