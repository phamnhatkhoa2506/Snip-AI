//! Khảo sát mức độ hài lòng — 3 bậc (không hài lòng/hài lòng/rất hài lòng) +
//! 1 ô góp ý tự do. App CHỦ ĐỘNG hiện khảo sát này (không phải người dùng tự
//! tìm ra), nên lịch hiện phải "thông minh" để không làm phiền:
//!
//! 1. Không hiện ngay từ lần mở đầu — chỉ hiện SAU KHI đã dùng app thật sự
//!    (từ `ASK_THRESHOLD` lượt hỏi AI thành công trở lên, xem
//!    `record_successful_ask` — được gọi từ `history::history_save_turn`,
//!    nơi đã sẵn là tín hiệu "1 lượt hỏi AI vừa xong thành công").
//! 2. Đã khảo sát xong (`completed = true`) thì KHÔNG BAO GIỜ hiện lại.
//! 3. Bỏ qua (không phải khảo sát xong) thì hiện lại sau, nhưng phải cách ra
//!    `DISMISS_COOLDOWN_DAYS` để không hỏi dồn dập mỗi lần mở app.
//!
//! Lưu trạng thái (đã khảo sát/số lượt hỏi/lần bỏ qua gần nhất) trong 1 file
//! JSON local — CÙNG QUY ƯỚC ghi nguyên tử (file tạm + rename) như
//! `history.rs::save_index_atomic`, tránh file hỏng nếu app bị tắt đột ngột
//! đúng lúc đang ghi.
//!
//! Câu trả lời khảo sát THẬT SỰ (điểm + góp ý) gửi VỀ BACKEND (POST
//! /v1/survey, xem backend/src/index.ts) — không lưu local, vì mục đích là để
//! người phát triển app đọc được phản hồi thật từ người dùng, không phải chỉ
//! để tự app biết "đã hỏi chưa". Gửi được cả khi CHƯA đăng nhập Google (kèm
//! email nếu có đăng nhập, ẩn danh nếu không) — không nên chặn quyền góp ý
//! chỉ vì chưa đăng nhập.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

use crate::state::HttpClientState;

const STATE_FILE_NAME: &str = "survey.json";

/// Cần ít nhất từng này lượt hỏi AI thành công trước khi lần đầu hiện khảo
/// sát — vừa mở app xong hỏi ngay "có hài lòng không" là vô nghĩa, người dùng
/// chưa kịp trải nghiệm gì.
const ASK_THRESHOLD: u32 = 3;

/// Bỏ qua 1 lần thì phải cách ra từng này rồi mới hiện lại — đủ dài để không
/// làm phiền (không phải mở app nào cũng bị hỏi), đủ ngắn để vẫn thu được
/// phản hồi từ người dùng dùng app lâu dài.
const DISMISS_COOLDOWN_DAYS: u64 = 14;

#[derive(Serialize, Deserialize, Default)]
struct SurveyState {
    completed: bool,
    successful_asks: u32,
    /// `None` = chưa từng bị bỏ qua lần nào.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_dismissed_at_ms: Option<u64>,
}

fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    // Cùng thư mục Local (không Roaming) với history.rs — xem giải thích đầy
    // đủ ở `history::history_root` (tránh đồng bộ lên server trong môi trường
    // roaming profile trường học/văn phòng).
    let base = app.path().app_local_data_dir().map_err(|e| format!("Không lấy được thư mục dữ liệu app: {e}"))?;
    Ok(base.join(STATE_FILE_NAME))
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Không bao giờ lỗi ra ngoài — file chưa có (lần đầu chạy) hoặc hỏng đều coi
/// như trạng thái mặc định, giống cách `history::load_index` xử lý.
fn load_state(app: &AppHandle) -> SurveyState {
    let path = match state_path(app) {
        Ok(p) => p,
        Err(_) => return SurveyState::default(),
    };
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => SurveyState::default(),
    }
}

/// Ghi qua file tạm rồi rename — nguyên tử trên Windows (cùng ổ đĩa), tránh
/// file hỏng nếu app tắt đột ngột giữa lúc ghi. Xem giải thích đầy đủ ở
/// `history::save_index_atomic`.
fn save_state_atomic(app: &AppHandle, s: &SurveyState) -> Result<(), String> {
    let path = state_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Không tạo được thư mục cấu hình: {e}"))?;
    }
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(s).map_err(|e| format!("Lỗi mã hoá trạng thái khảo sát: {e}"))?;
    fs::write(&tmp_path, &json).map_err(|e| format!("Lỗi ghi file tạm: {e}"))?;
    fs::rename(&tmp_path, &path).map_err(|e| format!("Lỗi hoàn tất ghi file: {e}"))
}

/// Gọi từ `history::history_save_turn` mỗi khi 1 lượt hỏi AI vừa lưu thành
/// công — tín hiệu "đã dùng app thật sự" để tính vào `ASK_THRESHOLD`. Cố tình
/// KHÔNG trả lỗi ra ngoài (chỉ log) — đây là tính năng PHỤ, không được làm
/// hỏng luồng lưu lịch sử chính nếu lỡ ghi đĩa thất bại.
pub fn record_successful_ask(app: &AppHandle) {
    // Đã khảo sát xong rồi thì khỏi tăng đếm nữa — không còn ý nghĩa gì.
    let mut s = load_state(app);
    if s.completed {
        return;
    }
    s.successful_asks = s.successful_asks.saturating_add(1);
    if let Err(e) = save_state_atomic(app, &s) {
        eprintln!("[snip-ai][survey] Lưu số lượt hỏi thất bại (bỏ qua): {e}");
    }
}

#[derive(Serialize)]
pub struct SurveyStatus {
    pub eligible: bool,
}

/// Cửa sổ Settings ("main") gọi lệnh này mỗi lần mở/hiện lại — quyết định có
/// nên hiện khảo sát ngay bây giờ hay không, theo đúng 3 quy tắc ở đầu file.
#[tauri::command]
pub fn survey_status(app: AppHandle) -> SurveyStatus {
    let s = load_state(&app);
    if s.completed || s.successful_asks < ASK_THRESHOLD {
        return SurveyStatus { eligible: false };
    }
    if let Some(dismissed_at) = s.last_dismissed_at_ms {
        let cooldown_ms = DISMISS_COOLDOWN_DAYS * 24 * 60 * 60 * 1000;
        if now_ms().saturating_sub(dismissed_at) < cooldown_ms {
            return SurveyStatus { eligible: false };
        }
    }
    SurveyStatus { eligible: true }
}

/// Bấm "Bỏ qua"/đóng khảo sát mà KHÔNG gửi trả lời — ghi lại thời điểm để áp
/// dụng cooldown, không đánh dấu `completed`.
#[tauri::command]
pub fn dismiss_survey(app: AppHandle) -> Result<(), String> {
    let mut s = load_state(&app);
    s.last_dismissed_at_ms = Some(now_ms());
    save_state_atomic(&app, &s)
}

/// Gửi câu trả lời thật lên backend rồi đánh dấu đã khảo sát xong (không bao
/// giờ hiện lại nữa, kể cả khi gửi thành công qua session/API key khác sau
/// này). Gửi được dù CHƯA đăng nhập Google — xem giải thích ở đầu file.
#[tauri::command]
pub async fn submit_survey(
    app: AppHandle,
    http: State<'_, HttpClientState>,
    rating: String,
    comment: String,
) -> Result<(), String> {
    const VALID: [&str; 3] = ["unhappy", "happy", "very_happy"];
    if !VALID.contains(&rating.as_str()) {
        return Err(format!("rating không hợp lệ: {rating}"));
    }

    let app_version = app.package_info().version.to_string();
    let body = serde_json::json!({
        "rating": rating,
        "comment": comment.trim(),
        "appVersion": app_version,
    });

    let url = format!("{}/v1/survey", crate::oauth::backend_base_url());
    let mut req = http.client.post(&url).json(&body);
    // Kèm session token nếu đã đăng nhập (để backend biết ai gửi) — KHÔNG bắt
    // buộc, thiếu thì backend vẫn nhận (lưu ẩn danh).
    if let Ok(token) = crate::oauth::read_session_token() {
        req = req.bearer_auth(token);
    }

    let resp = req.send().await.map_err(|e| format!("Lỗi gửi khảo sát: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gửi khảo sát thất bại (HTTP {status}): {text}"));
    }

    let mut s = load_state(&app);
    s.completed = true;
    save_state_atomic(&app, &s)
}
