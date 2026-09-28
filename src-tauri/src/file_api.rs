//! Gemini File API — dùng cho file đính kèm LỚN (xem attachments.rs), thay
//! vì nhồi base64 thẳng vào request (`inline_data`) như file nhỏ. File upload
//! qua đây được Google giữ sống ~48h và dùng lại được nhiều lần qua
//! `file_uri`, nên chỉ cần upload 1 lần/phiên (attachments.rs tự cache lại).
//!
//! 2 đường gọi, y hệt cấu trúc `GeminiAuth` dùng cho lệnh hỏi AI chính
//! (ai.rs):
//! - `Backend`: KHÔNG có API key thật trong tay (key nằm ở Cloudflare Worker)
//!   — phải nhờ backend tự upload hộ qua endpoint `/v1/gemini/upload` (xem
//!   backend/src/geminiProxy.ts), forward luôn qua Durable Object đã ghim vị
//!   trí như lệnh hỏi AI chính (tránh lỗi chặn vùng y hệt).
//! - `Direct`: có API key thật trong tay — tự gọi thẳng Google theo đúng giao
//!   thức "resumable upload" 2 bước của File API (bắt đầu phiên upload rồi
//!   gửi dữ liệu), không qua backend.

use reqwest::Client;

use crate::ai::GeminiAuth;

/// Ngưỡng tự động: file NHỎ HƠN mức này vẫn gửi kiểu cũ (`inline_data`, base64
/// thẳng trong request) — đơn giản, đủ nhanh, không cần round-trip upload
/// riêng. Chỉ file LỚN HƠN mới đi qua File API. "Vài MB" theo yêu cầu gốc —
/// chọn 4MB (đủ lớn để không upload vặt cho ảnh chụp màn hình thường, đủ nhỏ
/// để base64 hoá không phình payload đáng kể).
pub const INLINE_THRESHOLD_BYTES: usize = 4 * 1024 * 1024;

/// Upload 1 file, trả về `file_uri` để dùng trong field `file_data` của
/// request `generateContent` (thay cho `inline_data`).
pub async fn upload_file(client: &Client, auth: &GeminiAuth, mime: &str, display_name: &str, bytes: &[u8]) -> Result<String, String> {
    match auth {
        GeminiAuth::Backend { token } => upload_via_backend(client, token, mime, display_name, bytes).await,
        GeminiAuth::Direct { api_key } => upload_direct(client, api_key, mime, display_name, bytes).await,
    }
}

/// Percent-encode tên file để nhét an toàn vào HTTP header — tên file có thể
/// chứa tiếng Việt có dấu/khoảng trắng, HeaderValue chỉ chấp nhận ASCII hiển
/// thị được. Backend tự `decodeURIComponent` lại (xem index.ts/geminiProxy.ts).
fn encode_header_value(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Đường BACKEND — forward toàn bộ việc upload (kể cả chọn key, ghim vị trí
/// chạy) sang Cloudflare Worker, giống hệt lý do lệnh hỏi AI chính không gọi
/// thẳng Google khi đã đăng nhập (xem GeminiAuth::Backend trong ai.rs).
async fn upload_via_backend(client: &Client, token: &str, mime: &str, display_name: &str, bytes: &[u8]) -> Result<String, String> {
    let url = format!("{}/v1/gemini/upload", crate::oauth::backend_base_url());
    let resp = client
        .post(&url)
        .bearer_auth(token)
        .header("x-gemini-mime", mime)
        .header("x-gemini-filename", encode_header_value(display_name))
        .body(bytes.to_vec())
        .send()
        .await
        .map_err(|e| format!("Lỗi upload file qua backend: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Backend từ chối upload file (HTTP {status}): {text}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("Lỗi đọc phản hồi upload file (backend): {e}"))?;
    json["fileUri"].as_str().map(|s| s.to_string()).ok_or_else(|| "Backend thiếu \"fileUri\" trong phản hồi upload".to_string())
}

/// Đường DIRECT (API key tự nhập, hiện không còn kích hoạt được từ UI — xem
/// giải thích ở `GeminiAuth::Direct` trong ai.rs) — tự gọi thẳng Google theo
/// đúng giao thức "resumable upload" 2 bước của File API:
/// 1. "start": báo trước kích thước/mime, Google trả về 1 URL upload riêng
///    cho phiên này (header `X-Goog-Upload-URL`).
/// 2. "upload, finalize": gửi thẳng bytes tới URL đó, nhận lại `file.uri`.
async fn upload_direct(client: &Client, api_key: &str, mime: &str, display_name: &str, bytes: &[u8]) -> Result<String, String> {
    let start_resp = client
        .post("https://generativelanguage.googleapis.com/upload/v1beta/files")
        .header("x-goog-api-key", api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", bytes.len().to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime)
        .json(&serde_json::json!({"file": {"display_name": display_name}}))
        .send()
        .await
        .map_err(|e| format!("Lỗi bắt đầu upload file lên Gemini: {e}"))?;

    if !start_resp.status().is_success() {
        let status = start_resp.status();
        let text = start_resp.text().await.unwrap_or_default();
        return Err(format!("Gemini từ chối bắt đầu upload file (HTTP {status}): {text}"));
    }

    let upload_url = start_resp
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .ok_or("Gemini không trả về địa chỉ upload (thiếu header X-Goog-Upload-URL)")?
        .to_string();

    let finish_resp = client
        .post(&upload_url)
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .header("Content-Length", bytes.len().to_string())
        .body(bytes.to_vec())
        .send()
        .await
        .map_err(|e| format!("Lỗi khi gửi dữ liệu file lên Gemini: {e}"))?;

    if !finish_resp.status().is_success() {
        let status = finish_resp.status();
        let text = finish_resp.text().await.unwrap_or_default();
        return Err(format!("Gemini từ chối nhận file (HTTP {status}): {text}"));
    }

    let json: serde_json::Value = finish_resp.json().await.map_err(|e| format!("Lỗi đọc phản hồi upload file: {e}"))?;
    json["file"]["uri"].as_str().map(|s| s.to_string()).ok_or_else(|| "Phản hồi upload file thiếu \"file.uri\"".to_string())
}
