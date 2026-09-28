// "Tự động chép vào clipboard sau khi chụp/quay" — bật/tắt được ở menu Cài
// đặt cửa sổ chính (xem settings.ts::autoCopyOnCapture + +page.svelte), frontend
// đọc cờ này và truyền qua tham số `auto_copy` cho các lệnh chụp/quay (xem
// commands.rs::crop_and_open_result/append_capture_to_session/
// start_region_recording). Làm ở PHÍA RUST (không phải gọi `writeImage` từ
// JS sau khi nhận base64) để khỏi phải base64-hoá rồi gửi qua lại IPC — ảnh/
// video đã có sẵn dạng bytes ngay trong Rust.
//
// ẢNH: dùng thẳng `tauri-plugin-clipboard-manager` (đã là dependency sẵn có,
// dùng cho nút "Chép" câu trả lời text) — clipboard ảnh trên Windows là định
// dạng CHUẨN (bitmap), paste được vào hầu hết mọi nơi (Word, Paint, chat...).
//
// VIDEO: KHÔNG có định dạng clipboard chuẩn nào cho dữ liệu video thô — cách
// duy nhất để "dán được video ra chỗ khác" là chép clipboard kiểu FILE COPY
// (giống bấm Ctrl+C lên 1 file trong Explorer, dán ra nơi khác sẽ dán ra
// đúng file đó). Không có crate Rust thuần nào trong dependency hiện tại làm
// được việc này (clipboard-manager plugin chỉ hỗ trợ text/html/ảnh) — dùng
// PowerShell có sẵn trên mọi máy Windows (`Set-Clipboard -LiteralPath`, lệnh
// dựng sẵn từ PowerShell 5.0+) thay vì thêm hẳn 1 dependency mới chỉ cho việc
// này.
use std::path::Path;
use std::process::Command;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// CREATE_NO_WINDOW — không cho PowerShell hiện cửa sổ console đen nhoáng lên
/// rồi tắt (mặc định `Command::new` trên Windows vẫn tạo console ẩn nhưng có
/// thể thấy chớp cửa sổ với 1 số cấu hình).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Giải mã PNG -> RGBA thô rồi ghi vào clipboard ảnh hệ thống. Lỗi chỉ log
/// lại (không phải lỗi CHÍNH của lượt chụp) — chụp ảnh thành công vẫn tính là
/// thành công dù bước chép-thêm-vào-clipboard này có trục trặc.
pub fn copy_image_to_clipboard(app: &AppHandle, png_bytes: &[u8]) {
    let decoded = match image::load_from_memory(png_bytes) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("[snip-ai] Tự động chép ảnh: lỗi giải mã PNG: {e}");
            return;
        }
    };
    let (width, height) = decoded.dimensions();
    let rgba = decoded.into_raw();
    let image = tauri::image::Image::new_owned(rgba, width, height);
    match app.clipboard().write_image(&image) {
        Ok(()) => eprintln!("[snip-ai] Tự động chép ảnh vào clipboard: OK ({width}x{height})"),
        Err(e) => eprintln!("[snip-ai] Tự động chép ảnh vào clipboard: lỗi {e}"),
    }
}

/// Chép 1 file (video vừa quay xong) vào clipboard kiểu FILE COPY — gọi TRƯỚC
/// khi xoá file tạm (xoá xong thì tham chiếu trong clipboard trỏ tới file
/// không còn tồn tại, dán ra chỗ khác sẽ lỗi). Do vậy: bật tính năng này thì
/// file MP4 tạm KHÔNG bị xoá sau khi quay (xem record.rs) — đánh đổi chấp
/// nhận được vì người dùng đã CHỦ ĐỘNG bật tính năng này.
pub fn copy_video_file_to_clipboard(path: &Path) {
    let Some(path_str) = path.to_str() else {
        eprintln!("[snip-ai] Tự động chép video: đường dẫn chứa ký tự không hợp lệ");
        return;
    };
    // Escape dấu nháy đơn kiểu PowerShell ('' thay cho ') — đường dẫn thực tế
    // hiếm khi có dấu nháy đơn (nằm trong thư mục temp hệ thống) nhưng vẫn xử
    // lý phòng trường hợp tên người dùng Windows có ký tự lạ.
    let escaped = path_str.replace('\'', "''");
    let script = format!("Set-Clipboard -LiteralPath '{escaped}'");

    #[allow(unused_mut)]
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.output() {
        Ok(out) if out.status.success() => {
            eprintln!("[snip-ai] Tự động chép video vào clipboard: OK ({path_str})");
        }
        Ok(out) => {
            eprintln!(
                "[snip-ai] Tự động chép video vào clipboard: PowerShell lỗi: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(e) => eprintln!("[snip-ai] Tự động chép video vào clipboard: không chạy được PowerShell: {e}"),
    }
}
