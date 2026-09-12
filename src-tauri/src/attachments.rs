//! Đính kèm tài liệu gốc (ảnh/PDF) NGOÀI ảnh/video chính đã chụp — GIAI ĐOẠN
//! 1 của tính năng này. Chỉ hỗ trợ ẢNH (PNG/JPG/WEBP) và PDF — cả 2 đều được
//! Gemini đọc THẲNG qua `inline_data` giống ảnh chụp màn hình, không cần bóc
//! tách/convert gì cả (Gemini tự đọc chữ + hình trong PDF nhiều trang).
//!
//! DOCX/XLSX/PPTX CỐ TÌNH CHƯA hỗ trợ — Gemini KHÔNG đọc thẳng được các định
//! dạng Office này qua inline_data (khác PDF). Muốn hỗ trợ phải trích xuất
//! text ra trước (cần thêm crate đọc riêng từng định dạng, mất bố cục/hình
//! trong file) hoặc convert sang PDF (cần LibreOffice/công cụ ngoài — quá
//! nặng cho 1 app desktop nhẹ). Để dành giai đoạn sau nếu người dùng thật sự
//! cần.
//!
//! KHÔNG lưu vào lịch sử (history.rs) — cùng cách đơn giản hoá đã áp dụng
//! cho chuỗi ảnh/video (chỉ lưu media MỚI NHẤT, không lưu cả chuỗi): lịch sử
//! chỉ là "ảnh chụp nhanh" lúc lưu, không phải bản sao đầy đủ của phiên.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// Trần kích thước 1 file — Gemini giới hạn payload inline tổng cộng khoảng
/// 20MB (base64 hoá còn phình thêm ~33% nữa so với dung lượng gốc), kẹp thấp
/// hơn hẳn cho an toàn thay vì cố bám sát mức trần thật của Google.
const MAX_FILE_BYTES: u64 = 15 * 1024 * 1024;

/// Trần số lượng file đính kèm — PDF nặng hơn hẳn 1 ảnh chụp màn hình
/// thường, không nên để phình vô hạn như `MAX_CHAIN_ITEMS` (8) của ảnh/video.
pub const MAX_ATTACHMENTS: usize = 3;

fn mime_of(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_lowercase().as_str() {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn file_name_of(path: &Path, fallback: &str) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| fallback.to_string())
}

#[derive(Serialize, Clone)]
pub struct AttachmentMeta {
    pub name: String,
    pub mime: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
}

/// Đọc + kiểm tra + lưu các file đã chọn (đường dẫn tuyệt đối, lấy từ hộp
/// thoại chọn file phía frontend — xem `@tauri-apps/plugin-dialog`). Đọc
/// bằng `std::fs` thẳng trong Rust — KHÔNG qua capability fs của Tauri (chỉ
/// gate lệnh gọi từ JS, không áp cho std::fs gọi nội bộ trong command của
/// chính app), nên không cần thêm quyền fs nào ngoài quyền mở hộp thoại.
#[tauri::command]
pub fn attach_files_to_session(app: AppHandle, state: State<'_, AppState>, window_label: String, paths: Vec<String>) -> Result<(), String> {
    let mut sessions = state.attachment_sessions.lock().unwrap();
    let list = sessions.entry(window_label.clone()).or_default();

    for path_str in paths {
        if list.len() >= MAX_ATTACHMENTS {
            return Err(format!("Chỉ đính kèm được tối đa {MAX_ATTACHMENTS} file mỗi phiên."));
        }
        let path = Path::new(&path_str);
        let name = file_name_of(path, &path_str);
        let Some(mime) = mime_of(path) else {
            return Err(format!("Định dạng không hỗ trợ: \"{name}\" — hiện chỉ nhận ảnh (PNG/JPG/WEBP) và PDF."));
        };
        let bytes = std::fs::read(path).map_err(|e| format!("Không đọc được file \"{name}\": {e}"))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!(
                "File \"{name}\" quá lớn ({:.1}MB) — giới hạn {}MB mỗi file.",
                bytes.len() as f64 / 1024.0 / 1024.0,
                MAX_FILE_BYTES / 1024 / 1024,
            ));
        }
        list.push((bytes, mime.to_string(), name));
    }

    drop(sessions);
    // Dùng chung sự kiện với chuỗi ảnh/video ("+ Chụp thêm bước") — cả 2 đều
    // là "nội dung đính kèm vào phiên vừa đổi", UI chỉ cần nạp lại là đủ,
    // không cần phân biệt sự kiện riêng cho từng loại.
    let _ = app.emit_to(&window_label, "ai:chain-updated", ());
    Ok(())
}

#[tauri::command]
pub fn get_attachment_list(state: State<'_, AppState>, window_label: String) -> Vec<AttachmentMeta> {
    state
        .attachment_sessions
        .lock()
        .unwrap()
        .get(&window_label)
        .map(|list| {
            list.iter()
                .map(|(bytes, mime, name)| AttachmentMeta { name: name.clone(), mime: mime.clone(), size_bytes: bytes.len() as u64 })
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub fn remove_attachment_from_session(app: AppHandle, state: State<'_, AppState>, window_label: String, index: usize) -> Result<(), String> {
    let mut sessions = state.attachment_sessions.lock().unwrap();
    let list = sessions.get_mut(&window_label).ok_or("Không tìm thấy phiên này")?;
    if index >= list.len() {
        return Err("Chỉ số file không hợp lệ".into());
    }
    list.remove(index);
    drop(sessions);
    let _ = app.emit_to(&window_label, "ai:chain-updated", ());
    Ok(())
}

/// Dùng nội bộ trong ai.rs — trả về (base64, mime_type, tên file) của TOÀN
/// BỘ file đính kèm của phiên, đúng thứ tự đã thêm.
pub fn get_attachment_chain_base64(state: &State<'_, AppState>, window_label: &str) -> Vec<(String, String, String)> {
    state
        .attachment_sessions
        .lock()
        .unwrap()
        .get(window_label)
        .map(|list| list.iter().map(|(bytes, mime, name)| (STANDARD.encode(bytes), mime.clone(), name.clone())).collect())
        .unwrap_or_default()
}
