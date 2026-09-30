//! Ghi file XUẤT RA (CSV/Excel/Word/PNG...) do người dùng chọn nơi lưu qua
//! hộp thoại "Lưu file" (`@tauri-apps/plugin-dialog`, hàm `save()`) — frontend
//! tự dựng bytes đúng định dạng (xlsx qua `exceljs`, docx qua `docx`, PNG từ
//! canvas/SVG...) rồi gửi base64 sang đây để ghi thẳng ra đĩa.
//!
//! Đọc/ghi bằng `std::fs` thẳng trong Rust — KHÔNG qua capability fs của
//! Tauri (chỉ gate lệnh gọi TỪ JS qua plugin, không áp cho std::fs gọi nội bộ
//! trong command của chính app) — cùng kỹ thuật đã dùng ở `attachments.rs`,
//! nên không cần khai báo thêm quyền fs nào ngoài quyền mở hộp thoại lưu file
//! (`dialog:allow-save`, xem capabilities/default.json).
use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Đuôi file app THỰC SỰ xuất ra (xem exportFile.ts/exportImage.ts). Lệnh này
/// gọi được từ JS với đường dẫn BẤT KỲ — nếu có lỗ XSS nào lọt qua, kẻ tấn
/// công sẽ ghi được file tuỳ ý (VD .bat/.lnk vào thư mục Startup). Giới hạn
/// đuôi file loại bỏ hẳn lớp tấn công đó mà không ảnh hưởng người dùng thật.
const ALLOWED_EXTENSIONS: &[&str] = &["csv", "xlsx", "docx", "png", "jpg", "jpeg", "svg", "pdf", "txt", "md", "json"];

const MAX_EXPORT_BYTES: usize = 200 * 1024 * 1024;

fn validate_export_path(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("Đường dẫn file xuất phải là đường dẫn đầy đủ.".into());
    }
    let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("Định dạng file xuất không được hỗ trợ (\".{ext}\")."));
    }
    // Không cho ghi vào thư mục Startup của Windows (tự chạy mỗi lần đăng nhập).
    if path.to_ascii_lowercase().replace('/', "\\").contains("\\start menu\\programs\\startup") {
        return Err("Không được lưu vào thư mục khởi động của Windows.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn write_export_file(path: String, data_b64: String) -> Result<(), String> {
    validate_export_path(&path)?;
    let bytes = STANDARD.decode(&data_b64).map_err(|e| format!("Dữ liệu file xuất bị lỗi (base64 hỏng): {e}"))?;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err("File xuất quá lớn.".into());
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("Không ghi được file \"{path}\": {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_export_path as v;

    #[test]
    fn allows_normal_exports_and_blocks_dangerous_ones() {
        assert!(v(r"C:\Users\a\Documents\bao-cao.xlsx").is_ok());
        assert!(v(r"D:\x\ANH.PNG").is_ok());
        assert!(v(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\x.csv").is_err());
        assert!(v(r"C:\Users\a\evil.bat").is_err());
        assert!(v(r"C:\Users\a\evil.exe").is_err());
        assert!(v(r"C:\Users\a\noext").is_err());
        assert!(v("relative.csv").is_err());
    }
}
