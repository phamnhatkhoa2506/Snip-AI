//! Đính kèm tài liệu gốc (ảnh/PDF) NGOÀI ảnh/video chính đã chụp — GIAI ĐOẠN
//! 1 của tính năng này. Chỉ hỗ trợ ẢNH (PNG/JPG/WEBP) và PDF — cả 2 đều được
//! Gemini đọc THẲNG qua `inline_data`/`file_data` giống ảnh chụp màn hình,
//! không cần bóc tách/convert gì cả (Gemini tự đọc chữ + hình trong PDF
//! nhiều trang).
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
//!
//! NGƯỠNG TỰ ĐỘNG inline vs File API (xem file_api.rs): file NHỎ vẫn gửi
//! thẳng base64 (`inline_data`) như trước — đơn giản, đủ nhanh, không cần
//! round-trip upload riêng. File LỚN đi qua Gemini File API (upload 1 lần,
//! cache lại `file_uri` để tái dùng ~48h thay vì nhồi base64 khổng lồ vào
//! MỌI lượt hỏi trong cùng phiên).

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, AttachmentEntry};

/// Trần kích thước 1 file — nâng lên đáng kể so với trước (15MB) vì file lớn
/// giờ đi qua Gemini File API thay vì nhồi thẳng base64 vào request (không
/// còn bị chặn bởi trần payload inline ~20MB của Google nữa). Vẫn kẹp 1 mức
/// hợp lý cho 1 app desktop chụp màn hình/đính PDF — không cần cho phép tới
/// sát trần thật của File API (Google cho tới 2GB/file).
const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

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
        list.push(AttachmentEntry { bytes, mime: mime.to_string(), name, file_uri: None, key_id: None });
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
                .map(|e| AttachmentMeta { name: e.name.clone(), mime: e.mime.clone(), size_bytes: e.bytes.len() as u64 })
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

/// Ảnh chụp nhanh (snapshot, `Clone`) TOÀN BỘ file đính kèm của phiên tại
/// thời điểm gọi — KHÔNG giữ khoá `attachment_sessions` xuyên suốt lúc chờ
/// mạng (`resolve_attachments_for_request` bên dưới có thể phải `.await` 1
/// lượt upload File API), tránh chặn các thao tác đính/xoá file khác trong
/// lúc đang hỏi AI.
fn snapshot_attachments(state: &State<'_, AppState>, window_label: &str) -> Vec<AttachmentEntry> {
    state.attachment_sessions.lock().unwrap().get(window_label).cloned().unwrap_or_default()
}

/// Ghi lại `file_uri` vừa upload được cho ĐÚNG file đó — so khớp theo
/// tên + kích thước (đơn giản hơn thêm hẳn 1 id riêng cho từng file, đủ phân
/// biệt trong giới hạn tối đa MAX_ATTACHMENTS file/phiên). Lần hỏi SAU trong
/// cùng phiên đọc lại cache này, KHÔNG upload lại (Google giữ file sống ~48h,
/// xem file_api.rs).
fn cache_uploaded_file_uri(state: &State<'_, AppState>, window_label: &str, name: &str, size: usize, up: &crate::file_api::UploadedFile) {
    if let Some(list) = state.attachment_sessions.lock().unwrap().get_mut(window_label) {
        if let Some(entry) = list.iter_mut().find(|e| e.name == name && e.bytes.len() == size) {
            entry.file_uri = Some(up.uri.clone());
            entry.key_id = up.key_id.clone();
        }
    }
}

/// Xoá cache file đã upload của cả phiên (hết hạn 48h, key không còn...) — lần
/// hỏi SAU sẽ upload lại từ bytes gốc còn giữ trong RAM.
pub fn invalidate_uploads(state: &State<'_, AppState>, window_label: &str) {
    if let Some(list) = state.attachment_sessions.lock().unwrap().get_mut(window_label) {
        for e in list.iter_mut() {
            e.file_uri = None;
            e.key_id = None;
        }
    }
}

/// Key mà các file đã upload của request này đang gắn — gửi kèm request hỏi
/// AI (header x-snap-key-id) để backend ghim đúng key.
pub fn pinned_key_id(parts: &[AttachmentPart]) -> Option<String> {
    parts.iter().find_map(|p| match p {
        AttachmentPart::FileRef { key_id, .. } => key_id.clone(),
        _ => None,
    })
}

pub fn has_file_refs(parts: &[AttachmentPart]) -> bool {
    parts.iter().any(|p| matches!(p, AttachmentPart::FileRef { .. }))
}

/// 1 phần đính kèm ĐÃ QUYẾT ĐỊNH xong cách gửi cho Gemini — `Inline` (file
/// nhỏ, base64 thẳng trong request, như hành vi cũ) hoặc `FileRef` (file
/// lớn, tham chiếu qua `file_uri` đã upload qua File API). ai.rs chỉ cần
/// match 2 nhánh này để build đúng field JSON (`inline_data` hay `file_data`),
/// không cần biết logic ngưỡng/cache nằm ở đâu.
pub enum AttachmentPart {
    Inline { b64: String, mime: String, name: String },
    FileRef { uri: String, mime: String, name: String, key_id: Option<String> },
}

/// Quyết định cách gửi TỪNG file đính kèm của phiên cho lượt hỏi hiện tại —
/// file nhỏ hơn `file_api::INLINE_THRESHOLD_BYTES` gửi inline như cũ, file
/// lớn hơn thì upload qua File API (dùng lại `file_uri` đã cache nếu có).
/// Upload lỗi (mạng/Google từ chối...) KHÔNG làm hỏng cả lượt hỏi — fallback
/// về gửi inline như file nhỏ (chấp nhận payload lớn hơn 1 lần còn hơn hỏng
/// hẳn câu hỏi vì lỗi phụ ở bước tối ưu).
pub async fn resolve_attachments_for_request(
    client: &reqwest::Client,
    auth: &crate::ai::GeminiAuth,
    state: &State<'_, AppState>,
    window_label: &str,
) -> Vec<AttachmentPart> {
    let snapshot = snapshot_attachments(state, window_label);
    let mut out = Vec::with_capacity(snapshot.len());

    // Mọi file của 1 request phải thuộc CÙNG 1 key (1 request không tham
    // chiếu được file của 2 project Google khác nhau): lấy key của file đã
    // upload đầu tiên làm chuẩn, file nào gắn key khác thì upload lại.
    let mut pin: Option<String> = snapshot
        .iter()
        .find(|e| e.bytes.len() > crate::file_api::INLINE_THRESHOLD_BYTES && e.file_uri.is_some())
        .and_then(|e| e.key_id.clone());

    for entry in snapshot {
        if entry.bytes.len() <= crate::file_api::INLINE_THRESHOLD_BYTES {
            out.push(AttachmentPart::Inline { b64: STANDARD.encode(&entry.bytes), mime: entry.mime, name: entry.name });
            continue;
        }

        if let Some(uri) = entry.file_uri.clone() {
            if entry.key_id == pin {
                out.push(AttachmentPart::FileRef { uri, mime: entry.mime, name: entry.name, key_id: entry.key_id.clone() });
                continue;
            }
            // Khác key chuẩn -> rơi xuống upload lại (ghim đúng key chuẩn).
        }

        match crate::file_api::upload_file(client, auth, &entry.mime, &entry.name, &entry.bytes, pin.as_deref()).await {
            Ok(up) => {
                cache_uploaded_file_uri(state, window_label, &entry.name, entry.bytes.len(), &up);
                if pin.is_none() {
                    pin = up.key_id.clone();
                }
                out.push(AttachmentPart::FileRef { uri: up.uri, mime: entry.mime, name: entry.name, key_id: up.key_id });
            }
            Err(e) => {
                eprintln!("[snip-ai][attachments] Upload File API lỗi cho \"{}\": {e} — gửi inline thay thế", entry.name);
                out.push(AttachmentPart::Inline { b64: STANDARD.encode(&entry.bytes), mime: entry.mime, name: entry.name });
            }
        }
    }

    out
}
