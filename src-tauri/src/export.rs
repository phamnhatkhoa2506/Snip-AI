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

#[tauri::command]
pub fn write_export_file(path: String, data_b64: String) -> Result<(), String> {
    let bytes = STANDARD.decode(&data_b64).map_err(|e| format!("Dữ liệu file xuất bị lỗi (base64 hỏng): {e}"))?;
    std::fs::write(&path, &bytes).map_err(|e| format!("Không ghi được file \"{path}\": {e}"))?;
    Ok(())
}
