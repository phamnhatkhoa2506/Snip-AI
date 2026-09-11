use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::atomic::Ordering;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
};

use crate::capture;
use crate::state::{AppState, MonitorBounds, PendingRecordResult, MAX_CHAIN_ITEMS};

const OVERLAY_LABEL: &str = "overlay";
/// Label cửa sổ thanh công cụ nổi lúc đang quay video (Start/Stop/timer,
/// giống Snipping Tool) — cũng là SINGLETON như overlay, hợp lý vì chỉ quay
/// được 1 phiên tại 1 thời điểm (đã enforce ở `state.recording_stop_flag`).
const TOOLBAR_LABEL: &str = "record-toolbar";
/// Tiền tố label cho MỌI cửa sổ "Kết quả AI" — mỗi lần snip tạo 1 label MỚI
/// (VD "result-3"), không dùng chung 1 label cố định như trước nữa. Nhờ vậy
/// snip nhiều lần liên tiếp mà không đóng popup cũ sẽ mở NHIỀU cửa sổ độc
/// lập, thay vì cửa sổ mới đóng mất cửa sổ cũ (bug đã gặp thực tế).
pub const RESULT_LABEL_PREFIX: &str = "result-";
/// Tiền tố label cho cửa sổ "Kết quả AI" của 1 phiên QUAY VIDEO — dùng CHUNG
/// route "result" (result/+page.svelte tự nhận biết mình đang ở phiên video
/// hay ảnh qua tiền tố label, xem `getCurrentWindow().label` ở đó), chỉ khác
/// tiền tố để lib.rs biết dọn đúng session map lúc đóng cửa sổ.
pub const RECORD_LABEL_PREFIX: &str = "record-";

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
///
/// Dùng CHUNG cho cả snip ảnh LẪN quay video — 2 tính năng đều cần "kéo chọn
/// vùng trên nền ảnh chụp đã làm tối" giống hệt nhau, chỉ khác BƯỚC SAU khi
/// thả chuột (ảnh: crop ngay; video: bắt đầu quay theo vùng đó). `overlay_url`
/// quyết định overlay/+page.svelte biết mình đang ở chế độ nào (query param
/// `?mode=record`) để gọi đúng lệnh lúc thả chuột.
pub fn capture_and_open_overlay(app: &AppHandle, overlay_url: &str) -> Result<(), String> {
    // CHẶN TỪ ĐẦU nếu chưa đăng nhập — không mở overlay/cho chọn vùng gì cả.
    // Trước đây chỉ chặn ở cửa sổ Settings (không có nút "+ New"), nhưng phím
    // tắt toàn cục lại KHÔNG đi qua cửa sổ đó nên vẫn lọt: người dùng bấm
    // phím tắt, chọn vùng xong xuôi, mở cửa sổ "Kết quả AI", gõ câu hỏi rồi
    // MỚI biết bị chặn (lỗi "chưa có API key" trỏ tới 1 tính năng đã bị gỡ
    // khỏi UI — ngõ cụt thật sự). Chặn ở đây vì đây là điểm DUY NHẤT cả phím
    // tắt lẫn nút bấm UI đều đi qua trước khi mở bất kỳ cửa sổ chọn vùng nào.
    if !crate::oauth::is_logged_in() {
        // Hiện lại cửa sổ Settings để người dùng thấy ngay màn hình "Đăng
        // nhập để bắt đầu" thay vì im lặng không có gì xảy ra khi bấm phím tắt.
        if let Some(main_win) = app.get_webview_window(crate::MAIN_LABEL) {
            let _ = main_win.show();
            let _ = main_win.set_focus();
        }
        return Err("Cần đăng nhập Google trước khi dùng AI.".into());
    }

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
    let win = WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App(overlay_url.into()))
        .title("Snap AI Overlay")
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
    capture_and_open_overlay(&app, "overlay")
}

/// Cho phép kích hoạt quay video thủ công từ UI, không chỉ qua phím tắt —
/// cùng lý do với `trigger_capture` phía trên.
#[tauri::command]
pub async fn trigger_recording_from_ui(app: AppHandle) -> Result<(), String> {
    trigger_recording(&app)
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

    // Đóng overlay NGAY khi vừa nhận toạ độ vùng chọn — trước khi crop/resize
    // (việc nặng CPU, có thể mất vài trăm ms với vùng lớn). Đóng ngay cho phản
    // hồi tức thì, không để màn hình tối đứng yên trong lúc xử lý.
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.close();
    }

    // Vị trí/kích thước cửa sổ kết quả chỉ phụ thuộc x,y,height (toạ độ vùng
    // chọn) — KHÔNG cần đợi crop/resize ảnh xong mới tính được. Nên mở cửa sổ
    // kết quả NGAY Ở ĐÂY (trước khi xử lý ảnh), cửa sổ hiện trạng thái loading
    // trong lúc chờ, thay vì đợi xử lý ảnh xong mới mở cửa sổ — trước đây làm
    // theo thứ tự "xử lý ảnh rồi mới mở cửa sổ" khiến người dùng cảm thấy cửa
    // sổ kết quả "lâu hiện ra" hẳn (dù tổng thời gian xử lý là như nhau, chỉ
    // là được che bởi overlay trước đó, giờ lộ ra thành 1 khoảng chờ trống).
    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    let window_label = format!("{RESULT_LABEL_PREFIX}{session_id}");
    let crop_bounds_x = monitor.x + x as i32;
    let crop_bounds_y = monitor.y + y as i32;

    eprintln!("[snip-ai] mở cửa sổ kết quả NGAY (label={window_label}), ảnh sẽ nạp sau khi xử lý xong");
    open_result_window(&app, monitor, &window_label, crop_bounds_x, crop_bounds_y, height, session_id)?;
    // Chọn vùng xong, hiện lại Settings nếu vừa bị ẩn tạm — gọi SAU khi cửa sổ
    // kết quả đã show()+focus() để cửa sổ kết quả vẫn nằm trên cùng.
    restore_main_after_snip(&app);

    // Decode/crop/resize/encode ảnh là việc NẶNG CPU (đặc biệt bước resize
    // Lanczos3 với vùng crop lớn) — chạy đồng bộ ngay trong async command này
    // sẽ chiếm dụng luôn worker thread của Tokio runtime, khiến TOÀN BỘ app
    // (kể cả IPC/UI đang dùng chung runtime) bị khựng trong lúc xử lý. Đẩy
    // sang `spawn_blocking` để chạy trên thread pool riêng dành cho việc
    // CPU-bound, không chặn thread đang phục vụ IPC/window events.
    let cropped = tokio::task::spawn_blocking(move || capture::crop_png(&screenshot, x, y, width, height))
        .await
        .map_err(|e| format!("Lỗi nội bộ khi xử lý ảnh: {e}"))??;
    eprintln!("[snip-ai] crop xong, {} bytes PNG", cropped.len());

    state.crop_sessions.lock().unwrap().insert(window_label.clone(), vec![cropped]);

    // Cửa sổ kết quả đã mở TỪ TRƯỚC lúc ảnh chưa có trong `crop_sessions` —
    // báo cho nó biết ảnh vừa sẵn sàng để tự gọi lại `get_crop_image_base64`
    // (xem `loadCropImage()` + lắng nghe event này ở result/+page.svelte).
    let _ = app.emit_to(&window_label, "ai:crop-ready", ());

    eprintln!(
        "[snip-ai][timing] TỔNG crop_and_open_result (mở cửa sổ + crop + nạp ảnh): {:.0}ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

/// `pub(crate)` (không phải `pub` toàn bộ, không phải riêng file này) — cần
/// gọi được từ `record.rs` (mở cửa sổ "Kết quả AI" SAU KHI quay video xong,
/// xem giải thích ở `start_region_recording`), nhưng không cần expose ra
/// ngoài crate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_result_window(
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

    // Lệch (cascade) theo session_id để nhiều cửa sổ mở cùng 1 khu vực màn
    // hình không chồng khít lên nhau. 18px trước đây gần như không đáng kể so
    // với cửa sổ 480x340 (chỉ ~4% chiều rộng) — bấm snip vài lần liên tiếp là
    // cửa sổ mới che gần trọn cửa sổ cũ, người dùng tưởng ảnh "biến mất". Tăng
    // lên 60px (~12.5% chiều rộng) để phần header + vài dòng đầu của cửa sổ
    // cũ luôn còn lộ ra, thấy rõ "có nhiều popup" thay vì bị nuốt gần hết.
    let cascade = (session_id % 6) as f64 * (60.0 * scale);
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

/// Bấm phím tắt QUAY VIDEO — giống hệt snip ảnh ở bước ĐẦU (mở overlay, kéo
/// chọn vùng trên nền ảnh chụp đã làm tối, cùng màu khung chọn vùng), chỉ
/// khác Ở BƯỚC SAU khi thả chuột: overlay/+page.svelte tự biết gọi
/// `start_region_recording` thay vì `crop_and_open_result` nhờ query param
/// `?mode=record` trên URL overlay.
pub fn trigger_recording(app: &AppHandle) -> Result<(), String> {
    capture_and_open_overlay(app, "overlay?mode=record")
}

/// Bấm "+ Chụp thêm bước" trong lúc đang chat ở 1 phiên VIDEO — cùng ý tưởng
/// với `trigger_capture_for_session` nhưng cho quay video.
#[tauri::command]
pub async fn trigger_recording_for_session(app: AppHandle, window_label: String) -> Result<(), String> {
    capture_and_open_overlay(&app, &format!("overlay?mode=record&appendTo={window_label}"))
}

/// Người dùng đã kéo chọn xong vùng MUỐN QUAY trong overlay (chế độ
/// `?mode=record`). Khác `crop_and_open_result`: KHÔNG mở cửa sổ "Kết quả AI"
/// ngay — video chưa quay thì chưa có gì để hiện. Thay vào đó mở 1 thanh công
/// cụ nổi nhỏ (giống Snipping Tool: nút Dừng + đồng hồ đếm giờ + nút Huỷ),
/// video quay xong (đủ 30s hoặc bấm Dừng) thì thanh công cụ tự đóng và cửa sổ
/// "Kết quả AI" mới mở ra (xem `record.rs::start_recording` + cách nó gọi lại
/// `open_result_window` ở CUỐI phiên quay, không phải lúc bắt đầu).
#[tauri::command]
pub async fn start_region_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    // `Some(label)` = quay THÊM vào phiên đang mở (bấm "+ Chụp thêm bước" ở
    // 1 phiên video) — quay xong KHÔNG mở cửa sổ mới, chỉ push vào chuỗi của
    // đúng cửa sổ đó (xem finish() trong record.rs). `Option` nên luồng quay
    // bình thường (không truyền) vẫn y hệt trước đây.
    append_to: Option<String>,
) -> Result<(), String> {
    let monitor = {
        let guard = state.monitor_bounds.lock().unwrap();
        guard.ok_or("Chưa có thông tin màn hình")?
    };

    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.close();
    }

    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    let anchor_x = monitor.x + x as i32;
    let anchor_y = monitor.y + y as i32;

    open_recording_toolbar(&app, monitor, anchor_x, anchor_y)?;
    *state.recording_pending.lock().unwrap() =
        Some(PendingRecordResult { monitor, anchor_x, anchor_y, session_id, append_to });
    *state.recording_discard.lock().unwrap() = false;

    restore_main_after_snip(&app);

    eprintln!("[snip-ai] bắt đầu quay video vùng x={x} y={y} w={width} h={height} (session={session_id})");
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::record::start_recording(app_clone.clone(), Some((x, y, width, height))).await {
            eprintln!("[snip-ai] Lỗi bắt đầu quay: {e}");
            let _ = app_clone.emit_to(TOOLBAR_LABEL, "recording:error", e);
        }
    });

    Ok(())
}

/// Thanh công cụ nổi lúc đang quay — nhỏ, không viền, luôn nổi trên cùng,
/// đặt Ở GIỮA - SÁT MÉP TRÊN màn hình chính (giống vị trí thanh công cụ của
/// Windows Snipping Tool khi quay) — KHÔNG bám theo vùng vừa chọn, luôn cùng
/// 1 chỗ dễ tìm dù chọn vùng ở đâu trên màn hình.
fn open_recording_toolbar(
    app: &AppHandle,
    monitor: MonitorBounds,
    _anchor_x: i32,
    _anchor_y: i32,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let scale = *state.scale_factor.lock().unwrap();
    let scale = if scale > 0.0 { scale } else { 1.0 };

    let win_w = (220.0_f64 * scale).round();
    let win_h = (56.0_f64 * scale).round();
    let top_gap = (14.0_f64 * scale).round();

    let pos_x = monitor.x as f64 + (monitor.width as f64 - win_w) / 2.0;
    let pos_y = monitor.y as f64 + top_gap;

    if let Some(win) = app.get_webview_window(TOOLBAR_LABEL) {
        let _ = win.close();
    }
    let win = WebviewWindowBuilder::new(app, TOOLBAR_LABEL, WebviewUrl::App("record-toolbar".into()))
        .title("Đang quay")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(true)
        .transparent(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được thanh công cụ quay: {e}"))?;
    let _ = win.set_size(PhysicalSize::new(win_w, win_h));
    let _ = win.set_position(PhysicalPosition::new(pos_x, pos_y));
    let _ = win.show();
    let _ = win.set_focus();

    Ok(())
}

/// Bấm nút "X" trên thanh công cụ — HUỶ HẲN (khác nút Dừng: vẫn giữ video).
/// Set cờ huỷ TRƯỚC rồi mới báo dừng — record.rs kiểm tra cờ này lúc quay
/// vừa kết thúc để biết KHÔNG mở cửa sổ "Kết quả AI"/KHÔNG giữ video.
#[tauri::command]
pub fn cancel_recording(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    *state.recording_discard.lock().unwrap() = true;
    crate::record::stop_recording(app)
}

/// `window_label`: label của cửa sổ "Kết quả AI" đang gọi lệnh này (frontend
/// tự đọc qua `getCurrentWindow().label` rồi truyền vào) — xác định đúng ảnh
/// của PHIÊN đó, vì giờ nhiều cửa sổ có thể mở cùng lúc, mỗi cửa sổ 1 ảnh
/// khác nhau. 1 phiên có thể có NHIỀU ảnh (chuỗi snip) — trả về ảnh MỚI NHẤT
/// (dùng cho preview chính); muốn cả chuỗi thì dùng `get_crop_chain_base64`.
#[tauri::command]
pub fn get_crop_image_base64(state: State<'_, AppState>, window_label: String) -> Result<String, String> {
    let sessions = state.crop_sessions.lock().unwrap();
    let list = sessions
        .get(&window_label)
        .ok_or("Không tìm thấy ảnh cho phiên này (cửa sổ có thể đã bị đóng/dọn dẹp)")?;
    let bytes = list.last().ok_or("Phiên này chưa có ảnh nào")?;
    Ok(STANDARD.encode(bytes))
}

/// Toàn bộ chuỗi ảnh đã chụp cho phiên này, ĐÚNG THỨ TỰ đã chụp — dùng để vẽ
/// dải thumbnail nhiều ảnh khi phiên có từ 2 ảnh trở lên (xem "Chụp thêm bước").
#[tauri::command]
pub fn get_crop_chain_base64(state: State<'_, AppState>, window_label: String) -> Result<Vec<String>, String> {
    let sessions = state.crop_sessions.lock().unwrap();
    let list = sessions
        .get(&window_label)
        .ok_or("Không tìm thấy ảnh cho phiên này (cửa sổ có thể đã bị đóng/dọn dẹp)")?;
    Ok(list.iter().map(|b| STANDARD.encode(b)).collect())
}

/// Bấm "+ Chụp thêm bước" ngay trong lúc đang chat — mở overlay chọn vùng
/// (dùng chung `capture_and_open_overlay`, query param `appendTo` cho
/// overlay/+page.svelte biết gọi `append_capture_to_session` thay vì
/// `crop_and_open_result` lúc thả chuột). Có async vì có thể tạo cửa sổ
/// overlay mới, xem giải thích ở `trigger_capture`.
#[tauri::command]
pub async fn trigger_capture_for_session(app: AppHandle, window_label: String) -> Result<(), String> {
    capture_and_open_overlay(&app, &format!("overlay?appendTo={window_label}"))
}

/// Người dùng đã kéo chọn xong vùng MUỐN THÊM vào phiên `window_label` đang
/// mở — khác `crop_and_open_result`: KHÔNG mở cửa sổ mới, chỉ PUSH ảnh vừa
/// crop vào đúng chuỗi đã có rồi báo cho cửa sổ đó tự nạp lại (event
/// "ai:chain-updated", xem result/+page.svelte).
#[tauri::command]
pub async fn append_capture_to_session(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let screenshot = {
        let guard = state.screenshot_png.lock().unwrap();
        guard.clone().ok_or("Chưa có ảnh chụp màn hình nào")?
    };

    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = win.close();
    }
    restore_main_after_snip(&app);

    let cropped = tokio::task::spawn_blocking(move || capture::crop_png(&screenshot, x, y, width, height))
        .await
        .map_err(|e| format!("Lỗi nội bộ khi xử lý ảnh: {e}"))??;

    {
        let mut sessions = state.crop_sessions.lock().unwrap();
        let list = sessions.entry(window_label.clone()).or_default();
        list.push(cropped);
        // Trần chuỗi — vượt quá thì bỏ bớt ảnh CŨ NHẤT, giữ đúng MAX_CHAIN_ITEMS
        // ảnh gần nhất. Không chặn hẳn việc chụp thêm (khó hiểu với người dùng
        // hơn là tự động "trượt cửa sổ" như thế này).
        while list.len() > MAX_CHAIN_ITEMS {
            list.remove(0);
        }
    }

    eprintln!("[snip-ai] append_capture_to_session({window_label}): đã thêm 1 ảnh vào chuỗi");
    let _ = app.emit_to(&window_label, "ai:chain-updated", ());
    Ok(())
}

/// Label SINGLETON — chỉ 1 cửa sổ Lịch sử tại 1 thời điểm, gọi lại thì show
/// + focus cửa sổ cũ thay vì tạo cửa sổ mới chồng lên (khác cửa sổ "Kết quả
/// AI", vốn cố tình cho phép nhiều cái mở song song).
const HISTORY_LABEL: &str = "history";

/// `async fn` — cùng lý do với `trigger_capture`: lệnh này có thể phải TẠO
/// cửa sổ mới, gọi trực tiếp trong 1 command đồng bộ dễ tự-deadlock trên
/// Windows/WebView2 (xem giải thích chi tiết ở `trigger_capture`).
#[tauri::command]
pub async fn open_history_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(HISTORY_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(&app, HISTORY_LABEL, WebviewUrl::App("history".into()))
        .title("Lịch sử")
        .decorations(true)
        .inner_size(760.0, 560.0)
        .min_inner_size(480.0, 360.0)
        .resizable(true)
        .build()
        .map_err(|e| format!("Không mở được cửa sổ lịch sử: {e}"))?;
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}
