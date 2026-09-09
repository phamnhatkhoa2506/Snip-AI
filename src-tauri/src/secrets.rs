//! Quản lý API key an toàn qua OS keychain (Windows Credential Manager).
//!
//! Thiết kế bảo mật (thay cho cách cũ lưu plaintext trong localStorage):
//! - Key được ghi thẳng vào Credential Manager của Windows, mã hoá bởi OS và
//!   gắn với tài khoản Windows đang đăng nhập.
//! - Frontend CHỈ có thể: ghi key mới, hỏi "đã có key chưa", và xoá key.
//!   KHÔNG có lệnh nào trả key về cho JS — nên kể cả khi có lỗ hổng XSS trong
//!   webview (VD nội dung AI trả về chứa script độc), key vẫn không đọc được.
//! - Mọi lệnh gọi AI (ai.rs) tự đọc key từ keychain ở phía Rust.

use keyring::Entry;
use serde::Serialize;

/// Tên "service" trong Credential Manager; mỗi provider là 1 "user" riêng.
const SERVICE: &str = "snip-ai";

/// Danh sách provider hợp lệ — chặn frontend truyền chuỗi tuỳ ý vào keychain.
const VALID_PROVIDERS: [&str; 4] = ["nvidia", "openai", "anthropic", "gemini"];

fn validate(provider: &str) -> Result<(), String> {
    if VALID_PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(format!("Provider không hợp lệ: {provider}"))
    }
}

fn entry(provider: &str) -> Result<Entry, String> {
    validate(provider)?;
    Entry::new(SERVICE, provider).map_err(|e| format!("Không mở được keychain: {e}"))
}

/// Đọc key ở phía Rust (dùng nội bộ cho ai.rs) — KHÔNG expose ra frontend.
pub fn read_api_key(provider: &str) -> Result<String, String> {
    let key = entry(provider)?
        .get_password()
        .map_err(|_| format!("Chưa lưu API key cho {provider}. Mở Cài đặt để nhập key."))?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(format!("API key của {provider} rỗng. Mở Cài đặt để nhập lại."));
    }
    Ok(key)
}

#[derive(Serialize)]
pub struct KeyStatus {
    pub provider: String,
    pub has_key: bool,
    /// 4 ký tự cuối của key, chỉ để người dùng nhận diện mình đã lưu key nào
    /// (VD "…a3f9"). Không đủ để tái tạo key.
    pub hint: String,
}

#[tauri::command]
pub fn save_api_key(provider: String, api_key: String) -> Result<(), String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("API key rỗng.".into());
    }
    entry(&provider)?
        .set_password(key)
        .map_err(|e| format!("Không lưu được key vào keychain: {e}"))
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), String> {
    match entry(&provider)?.delete_credential() {
        Ok(()) => Ok(()),
        // Không có key sẵn thì coi như xoá thành công (idempotent)
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Không xoá được key: {e}")),
    }
}

/// Trạng thái key của tất cả provider — dùng để hiện "đã cấu hình ✓" trong UI
/// mà không cần trả key thật về frontend.
#[tauri::command]
pub fn api_key_statuses() -> Vec<KeyStatus> {
    VALID_PROVIDERS
        .iter()
        .map(|p| match read_api_key(p) {
            Ok(key) => {
                let hint = if key.len() > 4 {
                    format!("…{}", &key[key.len() - 4..])
                } else {
                    "…".to_string()
                };
                KeyStatus {
                    provider: p.to_string(),
                    has_key: true,
                    hint,
                }
            }
            Err(_) => KeyStatus {
                provider: p.to_string(),
                has_key: false,
                hint: String::new(),
            },
        })
        .collect()
}
