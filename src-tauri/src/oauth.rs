//! Đăng nhập Google (OAuth 2.0, luồng "installed app" + PKCE) — thay thế cho
//! việc người dùng tự nhập API key. Toàn bộ luồng:
//!
//! 1. App sinh cặp PKCE (`code_verifier` giữ bí mật ở app, `code_challenge`
//!    gửi công khai) + 1 `state` ngẫu nhiên (chống CSRF) + mở 1 cổng TCP tạm
//!    trên `127.0.0.1` (chọn ngẫu nhiên, do OS cấp) để nhận callback.
//! 2. Mở trình duyệt hệ thống tới trang đăng nhập Google, kèm `redirect_uri`
//!    trỏ về cổng tạm đó.
//! 3. Người dùng đăng nhập/đồng ý trên trình duyệt → Google redirect trình
//!    duyệt về `http://127.0.0.1:<port>/callback?code=...&state=...` → app
//!    (đang lắng nghe sẵn ở bước 1) nhận được request đó, đọc `code`.
//! 4. App gửi `code` + `code_verifier` cho BACKEND (không gửi thẳng cho
//!    Google) — backend mới là nơi giữ `client_secret`, đổi lấy `id_token`
//!    thật từ Google, rồi cấp lại 1 "session token" riêng của backend.
//! 5. App lưu session token đó vào Windows Credential Manager (giống cách
//!    lưu API key trước đây) — dùng làm `Authorization: Bearer <token>` cho
//!    mọi lệnh gọi AI sau này, KHÔNG cần biết API key Gemini thật là gì nữa.
//!
//! Vì sao KHÔNG đổi code lấy token trực tiếp từ app: `client_secret` của
//! Google OAuth Desktop app BẮT BUỘC dùng ở bước đổi code, nhưng không được
//! phép nhúng vào app phân phối cho người dùng (ai cũng lấy được từ file
//! .exe) — nên bước đó phải xảy ra ở backend, nơi bí mật thật sự giữ được kín.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use keyring::Entry;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::state::HttpClientState;

/// URL backend đã deploy trên Cloudflare Workers — giữ API key Gemini thật +
/// xử lý đăng nhập Google, xem `backend/` trong repo.
const BACKEND_BASE_URL: &str = "https://snip-ai-backend.mizuneko2311.workers.dev";

/// OAuth Client ID (loại "Desktop app") — KHÔNG bí mật, an toàn để nhúng
/// thẳng vào app (đối lập với Client Secret, chỉ backend giữ).
const GOOGLE_CLIENT_ID: &str = "172316423385-jn5bi3gq5trh5jvloiaba0jep2vnipra.apps.googleusercontent.com";

const SESSION_SERVICE: &str = "snip-ai-session";

fn session_entry(field: &str) -> Result<Entry, String> {
    Entry::new(SESSION_SERVICE, field).map_err(|e| format!("Không mở được keychain: {e}"))
}

fn save_session(token: &str, email: &str, picture: Option<&str>) -> Result<(), String> {
    session_entry("token")?
        .set_password(token)
        .map_err(|e| format!("Không lưu được session token: {e}"))?;
    session_entry("email")?
        .set_password(email)
        .map_err(|e| format!("Không lưu được email: {e}"))?;
    // Ảnh đại diện không phải lúc nào cũng có (tài khoản Google không đặt
    // ảnh) — không coi thiếu ảnh là lỗi, chỉ đơn giản không lưu gì.
    if let Some(pic) = picture {
        let _ = session_entry("picture").and_then(|e| e.set_password(pic).map_err(|err| err.to_string()));
    }
    Ok(())
}

/// Đọc session token ở phía Rust (dùng nội bộ cho ai.rs) — KHÔNG expose ra
/// frontend, giống nguyên tắc ở secrets.rs.
pub fn read_session_token() -> Result<String, String> {
    session_entry("token")?
        .get_password()
        .map_err(|_| "Chưa đăng nhập. Mở Cài đặt để đăng nhập Google.".to_string())
}

fn read_session_email() -> Option<String> {
    session_entry("email").ok().and_then(|e| e.get_password().ok())
}

fn read_session_picture() -> Option<String> {
    session_entry("picture").ok().and_then(|e| e.get_password().ok())
}

/// Sinh 1 chuỗi ngẫu nhiên URL-safe (base64url, không padding) từ N byte
/// ngẫu nhiên — dùng cho cả `code_verifier` (PKCE) lẫn `state` (chống CSRF).
fn random_url_safe(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `code_challenge` = base64url(SHA-256(code_verifier)) — theo đúng chuẩn
/// PKCE (RFC 7636), method "S256".
fn pkce_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Mở 1 cổng TCP tạm trên loopback, để OS tự chọn cổng trống (tránh xung đột
/// nếu người dùng có app khác đang chiếm cổng cố định).
fn open_loopback_listener() -> Result<(TcpListener, u16), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Không mở được cổng loopback: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok((listener, port))
}

/// Chờ ĐÚNG 1 kết nối tới (Google redirect trình duyệt về đây sau khi người
/// dùng đăng nhập xong), đọc dòng request đầu tiên để lấy `code`/`state` từ
/// query string, trả về 1 trang HTML thân thiện cho trình duyệt, rồi đóng cổng.
///
/// CHẠY BLOCKING (accept() chặn luồng gọi nó) — bắt buộc gọi trong
/// `spawn_blocking`, không được gọi trực tiếp trong async command.
fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    let (stream, _addr) = listener.accept().map_err(|e| format!("Lỗi chờ callback đăng nhập: {e}"))?;
    handle_callback(stream, expected_state)
}

fn handle_callback(mut stream: TcpStream, expected_state: &str) -> Result<String, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| format!("Lỗi đọc request callback: {e}"))?;

    // Dòng đầu dạng "GET /callback?code=...&state=... HTTP/1.1"
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or("Request callback không hợp lệ (thiếu path)")?;
    // Ghép thành URL đầy đủ để dùng `url` crate parse query string an toàn
    // (tự xử lý percent-decode), thay vì tự tay tách chuỗi dễ sai.
    let full_url = format!("http://127.0.0.1{path}");
    let parsed = url::Url::parse(&full_url).map_err(|e| format!("Không parse được callback URL: {e}"))?;

    let mut code = None;
    let mut state = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => {
                // Người dùng bấm "Từ chối" trên màn hình đồng ý của Google
                let _ = write_html_response(&mut stream, "Đã huỷ đăng nhập", "Bạn có thể đóng tab này.");
                return Err(format!("Người dùng từ chối đăng nhập ({})", v));
            }
            _ => {}
        }
    }

    let _ = write_html_response(&mut stream, "Đăng nhập Snap AI thành công ✅", "Bạn có thể đóng tab này và quay lại app.");

    let code = code.ok_or("Không nhận được authorization code từ Google")?;
    let state = state.ok_or("Thiếu tham số state trong callback — huỷ đăng nhập để an toàn")?;
    if state != expected_state {
        return Err("State không khớp — huỷ đăng nhập để an toàn (có thể bị tấn công CSRF)".into());
    }
    Ok(code)
}

fn write_html_response(stream: &mut TcpStream, title: &str, message: &str) -> std::io::Result<()> {
    let body = format!(
        "<html><body style=\"font-family:system-ui,sans-serif;text-align:center;padding-top:80px;\
        background:#0f1115;color:#e8e8ea\"><h2>{title}</h2><p style=\"color:#9a9aa2\">{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())
}

#[derive(serde::Deserialize)]
struct ExchangeResponse {
    #[serde(rename = "sessionToken")]
    session_token: String,
    email: String,
    picture: Option<String>,
}

/// Thông tin tài khoản đang đăng nhập — trả cho frontend hiện avatar góc trên
/// bên phải (email KHÔNG bao gồm session token thật, giữ đúng nguyên tắc
/// "không expose bí mật ra frontend").
#[derive(serde::Serialize)]
pub struct LoginStatus {
    pub email: String,
    pub picture: Option<String>,
}

/// Chạy toàn bộ luồng đăng nhập Google, trả về thông tin tài khoản nếu thành công.
///
/// `async fn` — mở trình duyệt + gọi HTTP tới backend là việc I/O, và bước
/// chờ callback (blocking `accept()`) được tách sang `spawn_blocking` để
/// không chặn async runtime (xem giải thích chi tiết ở `commands.rs` cho các
/// lệnh tương tự tạo cửa sổ/xử lý ảnh nặng).
#[tauri::command]
pub async fn start_google_login(app: AppHandle) -> Result<LoginStatus, String> {
    let (listener, port) = open_loopback_listener()?;
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let verifier = random_url_safe(32);
    let challenge = pkce_challenge(&verifier);
    let state = random_url_safe(16);

    let mut auth_url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
        .map_err(|e| format!("Lỗi nội bộ dựng URL đăng nhập: {e}"))?;
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", GOOGLE_CLIENT_ID)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", "openid email profile")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);

    app.opener()
        .open_url(auth_url.as_str(), None::<&str>)
        .map_err(|e| format!("Không mở được trình duyệt để đăng nhập: {e}"))?;

    let expected_state = state.clone();
    let code = tokio::task::spawn_blocking(move || wait_for_callback(listener, &expected_state))
        .await
        .map_err(|e| format!("Lỗi nội bộ khi chờ đăng nhập: {e}"))??;

    let client = &app.state::<HttpClientState>().client;
    let resp = client
        .post(format!("{BACKEND_BASE_URL}/v1/auth/google/exchange"))
        .json(&serde_json::json!({
            "code": code,
            "codeVerifier": verifier,
            "redirectUri": redirect_uri,
        }))
        .send()
        .await
        .map_err(|e| format!("Lỗi kết nối tới backend: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Đăng nhập thất bại (HTTP {status}): {text}"));
    }

    let parsed: ExchangeResponse = resp
        .json()
        .await
        .map_err(|e| format!("Lỗi đọc phản hồi từ backend: {e}"))?;

    save_session(&parsed.session_token, &parsed.email, parsed.picture.as_deref())?;
    Ok(LoginStatus { email: parsed.email, picture: parsed.picture })
}

/// Thông tin tài khoản đang đăng nhập, nếu có — dùng để hiện avatar/email ở
/// UI mà không cần trả session token thật về frontend.
#[tauri::command]
pub fn get_login_status() -> Option<LoginStatus> {
    let email = read_session_email()?;
    Some(LoginStatus { email, picture: read_session_picture() })
}

#[tauri::command]
pub fn logout() -> Result<(), String> {
    // Idempotent — không có session sẵn cũng coi như thành công, giống cách
    // `delete_api_key` xử lý ở secrets.rs.
    let _ = session_entry("token").and_then(|e| e.delete_credential().map_err(|err| err.to_string()));
    let _ = session_entry("email").and_then(|e| e.delete_credential().map_err(|err| err.to_string()));
    let _ = session_entry("picture").and_then(|e| e.delete_credential().map_err(|err| err.to_string()));
    Ok(())
}

/// URL backend, dùng chung ở ai.rs khi gọi AI qua backend thay vì gọi thẳng
/// Google (xem `ai.rs::ask_ai_gemini_via_backend`).
pub fn backend_base_url() -> &'static str {
    BACKEND_BASE_URL
}
