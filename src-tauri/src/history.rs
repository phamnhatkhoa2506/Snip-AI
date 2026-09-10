// Lịch sử ảnh/video + hội thoại AI đi kèm, lưu LOCAL trên máy — không đụng gì
// tới backend (Cloudflare Worker chỉ đứng giữa lúc gọi AI, không biết và
// không nên biết nội dung này). Thiết kế theo 3 nguyên tắc đã thống nhất:
//
// 1. Tự dọn theo hạn dùng (14 ngày HOẶC vượt 500MB) — không giữ vô thời hạn.
// 2. 1 ảnh/video + hội thoại của nó là MỘT KHỐI — xoá là xoá sạch cả khối,
//    không giữ lại nửa (xoá tay, xoá tự động, hay dọn mục mồ côi đều vậy).
// 3. File mất bên ngoài app (người dùng tự vào Explorer xoá) không được làm
//    app lỗi/crash — chỉ đánh dấu `media_missing`, để UI tự xử lý.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

/// Quá tuổi này (tính từ lúc lưu) là bị dọn tự động, bất kể dung lượng.
const MAX_AGE_DAYS: u64 = 14;
/// Tổng dung lượng media (ảnh+video, KHÔNG tính index.json — không đáng kể)
/// vượt mốc này thì dọn bớt từ mục CŨ NHẤT cho tới khi về dưới trần.
const MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
pub struct HistoryTurn {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "displayLabel")]
    pub display_label: Option<String>,
}

/// 1 bản ghi lịch sử = đúng những gì lưu trong `index.json`. `media_file` chỉ
/// là TÊN file trong thư mục `media/` (không phải đường dẫn đầy đủ) — để dời
/// cả thư mục `history/` sang máy khác vẫn tự hoạt động, không phụ thuộc
/// đường dẫn tuyệt đối lúc lưu.
#[derive(Serialize, Deserialize, Clone)]
pub struct HistoryItem {
    pub id: String,
    /// "image" | "video"
    pub kind: String,
    pub created_at: u64,
    pub media_file: String,
    pub model: String,
    pub turns: Vec<HistoryTurn>,
}

#[derive(Serialize)]
pub struct HistoryListEntry {
    pub id: String,
    pub kind: String,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub model: String,
    pub preview: String,
    #[serde(rename = "turnCount")]
    pub turn_count: usize,
    #[serde(rename = "mediaMissing")]
    pub media_missing: bool,
}

#[derive(Serialize)]
pub struct HistoryItemFull {
    pub id: String,
    pub kind: String,
    pub model: String,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub turns: Vec<HistoryTurn>,
    #[serde(rename = "mediaB64")]
    pub media_b64: Option<String>,
    #[serde(rename = "mediaMime")]
    pub media_mime: String,
    #[serde(rename = "mediaMissing")]
    pub media_missing: bool,
}

// ── Đường dẫn trên đĩa ───────────────────────────────────────────────────

/// %LOCALAPPDATA%\<identifier>\history — CỐ Ý dùng `app_local_data_dir`
/// (Local), KHÔNG dùng `app_data_dir` (Roaming/%APPDATA%). Đối tượng chính
/// của app là máy trong trường học/văn phòng, nhiều nơi bật "roaming
/// profile" (miền AD) — %APPDATA% ở những máy đó được ĐỒNG BỘ LÊN SERVER mỗi
/// lần đăng nhập/đăng xuất. Ảnh/video vài trăm MB nằm trong đó sẽ làm đăng
/// nhập/đăng xuất chậm hẳn và có thể bị IT chặn hẳn việc ghi. LocalAppData
/// không đồng bộ đi đâu — đúng chỗ cho dữ liệu chỉ có ý nghĩa trên máy này.
fn history_root(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_local_data_dir().map_err(|e| format!("Không lấy được thư mục dữ liệu app: {e}"))?;
    Ok(base.join("history"))
}

fn media_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join("media"))
}

fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(history_root(app)?.join("index.json"))
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

// ── Đọc/ghi index.json ──────────────────────────────────────────────────

/// Không bao giờ trả lỗi ra ngoài — lần đầu chạy chưa có file, hoặc file lỡ
/// hỏng (VD tắt máy đột ngột giữa lúc ghi ở phiên bản cũ trước khi có ghi
/// nguyên tử) đều coi như "chưa có lịch sử" thay vì làm app không mở được.
fn load_index(app: &AppHandle) -> Vec<HistoryItem> {
    let path = match index_path(app) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("[snip-ai][history] index.json hỏng, bỏ qua và bắt đầu lại: {e}");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// Ghi qua file tạm rồi `rename` — trên Windows, rename cùng ổ đĩa là thao
/// tác NGUYÊN TỬ (MoveFileExW). Nếu app bị tắt đột ngột (crash, mất điện)
/// đúng lúc đang ghi, người dùng luôn có 1 trong 2: file CŨ còn nguyên, hoặc
/// file MỚI đã nguyên vẹn — không bao giờ đọc phải file ghi dở rồi tưởng
/// lịch sử bị hỏng.
fn save_index_atomic(app: &AppHandle, items: &[HistoryItem]) -> Result<(), String> {
    let dir = history_root(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Không tạo được thư mục lịch sử: {e}"))?;
    let final_path = dir.join("index.json");
    let tmp_path = dir.join("index.json.tmp");
    let json = serde_json::to_vec_pretty(items).map_err(|e| format!("Lỗi mã hoá lịch sử: {e}"))?;
    fs::write(&tmp_path, &json).map_err(|e| format!("Lỗi ghi file lịch sử tạm: {e}"))?;
    fs::rename(&tmp_path, &final_path).map_err(|e| format!("Lỗi ghi file lịch sử: {e}"))?;
    Ok(())
}

/// Dọn theo 2 quy tắc, cả 2 đều ưu tiên xoá mục CŨ NHẤT trước — xoá đúng
/// nghĩa "xoá sạch cả khối" (ảnh/video + hội thoại cùng lúc), không có
/// đường nào chỉ xoá 1 nửa.
fn prune(app: &AppHandle, items: &mut Vec<HistoryItem>) {
    let dir = match media_dir(app) {
        Ok(d) => d,
        Err(_) => return,
    };
    let now = now_ms();
    let max_age_ms = MAX_AGE_DAYS * 24 * 60 * 60 * 1000;

    items.sort_by_key(|it| it.created_at);

    // 1) Quá hạn dùng.
    items.retain(|it| {
        let expired = now.saturating_sub(it.created_at) > max_age_ms;
        if expired {
            let _ = fs::remove_file(dir.join(&it.media_file));
        }
        !expired
    });

    // 2) Vượt trần dung lượng -> xoá bớt từ mục cũ nhất (đầu vec, đã sort tăng
    // dần ở trên) tới khi về dưới trần. File thiếu (đã mồ côi) tính là 0 byte
    // — không giúp gì việc hạ dung lượng nên vòng lặp tự bỏ qua, sẽ bị dọn ở
    // lượt "quá hạn dùng" của lần chạy sau khi đủ tuổi.
    let mut total: u64 =
        items.iter().map(|it| fs::metadata(dir.join(&it.media_file)).map(|m| m.len()).unwrap_or(0)).sum();
    while total > MAX_TOTAL_BYTES && !items.is_empty() {
        let removed = items.remove(0);
        let sz = fs::metadata(dir.join(&removed.media_file)).map(|m| m.len()).unwrap_or(0);
        let _ = fs::remove_file(dir.join(&removed.media_file));
        total = total.saturating_sub(sz);
    }
}

/// Gọi 1 lần lúc app khởi động (xem lib.rs::setup) — nạp lịch sử đã lưu vào
/// state trong RAM (tránh đọc/parse lại index.json ở MỌI lệnh gọi), đồng thời
/// dọn ngay 1 lượt (app có thể đã tắt rất lâu, quy tắc "14 ngày" cần được áp
/// dụng cả khi app không chạy, không chỉ lúc app đang mở).
pub fn init(app: &AppHandle) {
    let mut items = load_index(app);
    let before = items.len();
    prune(app, &mut items);
    if items.len() != before {
        if let Err(e) = save_index_atomic(app, &items) {
            eprintln!("[snip-ai][history] Lỗi lưu lịch sử sau khi dọn lúc khởi động: {e}");
        }
    }
    *app.state::<AppState>().history_index.lock().unwrap() = items;
}

// ── Tauri commands ───────────────────────────────────────────────────────

/// Gọi SAU MỖI lượt AI trả lời thành công trong cửa sổ "Kết quả AI" (cả ảnh
/// lẫn video). Lần gọi ĐẦU của 1 phiên (window_label) tạo bản ghi mới + copy
/// ảnh/video ra `media/`; các lần gọi SAU (hỏi tiếp trong cùng cửa sổ) chỉ
/// cập nhật lại `turns`, không ghi lại media (không đổi). Lỗi ở đây KHÔNG
/// được làm hỏng luồng hỏi-đáp chính — frontend coi lỗi này là phụ, chỉ log.
#[tauri::command]
pub fn history_save_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    model: String,
    turns: Vec<HistoryTurn>,
) -> Result<(), String> {
    let existing_id = state.history_ids.lock().unwrap().get(&window_label).cloned();

    let mut index = state.history_index.lock().unwrap();

    if let Some(id) = &existing_id {
        if let Some(item) = index.iter_mut().find(|it| &it.id == id) {
            item.turns = turns;
            item.model = model;
            return save_index_atomic(&app, &index);
        }
        // id đã lưu nhưng bản ghi không còn trong index (đã bị dọn tự động
        // trong lúc phiên vẫn đang mở) -> rơi xuống dưới, tạo lại như mới.
    }

    let (bytes, kind, ext) = {
        let crop = state.crop_sessions.lock().unwrap();
        if let Some(b) = crop.get(&window_label) {
            (b.clone(), "image", "png")
        } else {
            drop(crop);
            let video = state.video_sessions.lock().unwrap();
            match video.get(&window_label) {
                Some(b) => (b.clone(), "video", "mp4"),
                None => return Err("Không tìm thấy ảnh/video của phiên này để lưu lịch sử".into()),
            }
        }
    };

    let id = crate::record::uuid_like();
    let media_file = format!("{id}.{ext}");
    let dir = media_dir(&app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Không tạo được thư mục lưu ảnh/video: {e}"))?;
    fs::write(dir.join(&media_file), &bytes).map_err(|e| format!("Không ghi được file lịch sử: {e}"))?;

    index.insert(0, HistoryItem { id: id.clone(), kind: kind.into(), created_at: now_ms(), media_file, model, turns });
    state.history_ids.lock().unwrap().insert(window_label, id);

    prune(&app, &mut index);
    save_index_atomic(&app, &index)
}

#[tauri::command]
pub fn history_list(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<HistoryListEntry>, String> {
    let dir = media_dir(&app)?;
    let mut index = state.history_index.lock().unwrap();
    index.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(index
        .iter()
        .map(|it| {
            let preview = it
                .turns
                .iter()
                .find(|t| t.role == "user")
                .map(|t| t.display_label.clone().unwrap_or_else(|| truncate_chars(&t.content, 60)))
                .unwrap_or_else(|| "(không có nội dung)".into());
            HistoryListEntry {
                id: it.id.clone(),
                kind: it.kind.clone(),
                created_at: it.created_at,
                model: it.model.clone(),
                preview,
                turn_count: it.turns.len(),
                media_missing: !dir.join(&it.media_file).exists(),
            }
        })
        .collect())
}

#[tauri::command]
pub fn history_get(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<HistoryItemFull, String> {
    let index = state.history_index.lock().unwrap();
    let item = index.iter().find(|it| it.id == id).ok_or("Không tìm thấy mục lịch sử này (có thể đã bị xoá)")?;
    let dir = media_dir(&app)?;
    let mime = if item.kind == "video" { "video/mp4" } else { "image/png" };
    let (media_b64, media_missing) = match fs::read(dir.join(&item.media_file)) {
        Ok(bytes) => (Some(STANDARD.encode(bytes)), false),
        Err(_) => (None, true),
    };
    Ok(HistoryItemFull {
        id: item.id.clone(),
        kind: item.kind.clone(),
        model: item.model.clone(),
        created_at: item.created_at,
        turns: item.turns.clone(),
        media_b64,
        media_mime: mime.into(),
        media_missing,
    })
}

/// Xoá 1 mục — luôn xoá CẢ media LẪN hội thoại cùng lúc (đã thống nhất:
/// không có đường xoá nửa vời). Frontend tự lo phần "Hoàn tác" (trì hoãn gọi
/// lệnh này vài giây) — lệnh này thực thi là xoá thật, không có thùng rác.
#[tauri::command]
pub fn history_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let dir = media_dir(&app)?;
    let mut index = state.history_index.lock().unwrap();
    if let Some(pos) = index.iter().position(|it| it.id == id) {
        let removed = index.remove(pos);
        let _ = fs::remove_file(dir.join(&removed.media_file));
    }
    state.history_ids.lock().unwrap().retain(|_, v| *v != id);
    save_index_atomic(&app, &index)
}

/// Xoá SẠCH toàn bộ lịch sử — nút "dọn nhanh" cho máy dùng chung (phòng máy
/// trường học/văn phòng), không giấu trong Cài đặt.
#[tauri::command]
pub fn history_clear_all(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dir = media_dir(&app)?;
    let mut index = state.history_index.lock().unwrap();
    for it in index.iter() {
        let _ = fs::remove_file(dir.join(&it.media_file));
    }
    index.clear();
    state.history_ids.lock().unwrap().clear();
    save_index_atomic(&app, &index)
}
