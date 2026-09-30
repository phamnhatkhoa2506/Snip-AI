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
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder};

use crate::state::{AppState, MediaItem, MediaKind};

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
    /// "image" | "video" | "audio" | "chat" (hỏi bằng chữ, không media) |
    /// "live" (cuộc gọi giọng nói trực tiếp — chỉ xem lại, không tiếp tục được)
    pub kind: String,
    pub created_at: u64,
    /// RỖNG với "chat"/"live" (không có media) — mọi chỗ đụng tới file phải
    /// qua `media_len`/`remove_media`, không thì `dir.join("")` trỏ vào chính
    /// thư mục `media/` (metadata trả kích thước thư mục, remove_file lỗi).
    pub media_file: String,
    pub model: String,
    pub turns: Vec<HistoryTurn>,
    /// Thời lượng cuộc gọi live (giây); 0 với loại khác. `default` để đọc
    /// được index.json của bản cũ chưa có field này.
    #[serde(default)]
    pub duration_secs: u64,
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
    #[serde(rename = "durationSecs")]
    pub duration_secs: u64,
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
    #[serde(rename = "durationSecs")]
    pub duration_secs: u64,
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

/// Kích thước file media của 1 mục — 0 nếu mục không có media (tên rỗng) hoặc
/// file đã mất.
fn media_len(dir: &std::path::Path, media_file: &str) -> u64 {
    if media_file.is_empty() {
        return 0;
    }
    fs::metadata(dir.join(media_file)).map(|m| m.len()).unwrap_or(0)
}

fn remove_media(dir: &std::path::Path, media_file: &str) {
    if !media_file.is_empty() {
        let _ = fs::remove_file(dir.join(media_file));
    }
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
            remove_media(&dir, &it.media_file);
        }
        !expired
    });

    // 2) Vượt trần dung lượng -> xoá bớt từ mục cũ nhất (đầu vec, đã sort tăng
    // dần ở trên) tới khi về dưới trần. File thiếu (đã mồ côi) tính là 0 byte
    // — không giúp gì việc hạ dung lượng nên vòng lặp tự bỏ qua, sẽ bị dọn ở
    // lượt "quá hạn dùng" của lần chạy sau khi đủ tuổi.
    let mut total: u64 = items.iter().map(|it| media_len(&dir, &it.media_file)).sum();
    while total > MAX_TOTAL_BYTES && !items.is_empty() {
        let removed = items.remove(0);
        let sz = media_len(&dir, &removed.media_file);
        remove_media(&dir, &removed.media_file);
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
    // Tín hiệu "1 lượt hỏi AI vừa xong thành công" cho lịch hiện khảo sát hài
    // lòng (xem survey.rs) — tính NGAY ĐÂY, không phụ thuộc việc ghi lịch sử
    // bên dưới có thành công hay không (bản thân việc AI trả lời thành công
    // mới là điều đáng tính, không phải việc lưu đĩa).
    crate::survey::record_successful_ask(&app);

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

    // 1 phiên có thể có NHIỀU ảnh/video (chuỗi "+ Chụp thêm bước") — Lịch sử
    // (tính năng riêng, đơn giản hơn) chỉ lưu ẢNH/VIDEO MỚI NHẤT của chuỗi,
    // không lưu cả chuỗi. Xem lại đầy đủ chuỗi thì mở lại đúng cửa sổ "Kết
    // quả AI" đó trong lúc còn mở — Lịch sử chỉ là ảnh chụp nhanh lúc lưu.
    // Phiên "Hỏi AI" bằng chữ chưa có media nào -> lưu loại "chat", không kèm
    // file (trước đây báo lỗi và không lưu gì cả).
    let latest = {
        let sessions = state.media_sessions.lock().unwrap();
        sessions.get(&window_label).and_then(|list| list.last()).map(|m| (m.bytes.clone(), m.kind))
    };
    let id = crate::record::uuid_like();
    let (kind, media_file) = match latest {
        Some((bytes, media_kind)) => {
            let (kind, ext) = match media_kind {
                MediaKind::Image => ("image", "png"),
                MediaKind::Video => ("video", "mp4"),
                MediaKind::Audio => ("audio", "wav"),
            };
            let media_file = format!("{id}.{ext}");
            let dir = media_dir(&app)?;
            fs::create_dir_all(&dir).map_err(|e| format!("Không tạo được thư mục lưu ảnh/video: {e}"))?;
            fs::write(dir.join(&media_file), &bytes).map_err(|e| format!("Không ghi được file lịch sử: {e}"))?;
            (kind, media_file)
        }
        None => ("chat", String::new()),
    };

    index.insert(
        0,
        HistoryItem { id: id.clone(), kind: kind.into(), created_at: now_ms(), media_file, model, turns, duration_secs: 0 },
    );
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
                media_missing: !it.media_file.is_empty() && !dir.join(&it.media_file).exists(),
                duration_secs: it.duration_secs,
            }
        })
        .collect())
}

#[tauri::command]
pub fn history_get(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<HistoryItemFull, String> {
    let index = state.history_index.lock().unwrap();
    let item = index.iter().find(|it| it.id == id).ok_or("Không tìm thấy mục lịch sử này (có thể đã bị xoá)")?;
    let dir = media_dir(&app)?;
    let mime = match item.kind.as_str() {
        "video" => "video/mp4",
        "audio" => "audio/wav",
        _ => "image/png",
    };
    let (media_b64, media_missing) = if item.media_file.is_empty() {
        (None, false) // "chat"/"live": vốn không có media, không phải bị mất
    } else {
        match fs::read(dir.join(&item.media_file)) {
            Ok(bytes) => (Some(STANDARD.encode(bytes)), false),
            Err(_) => (None, true),
        }
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
        duration_secs: item.duration_secs,
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
        remove_media(&dir, &removed.media_file);
    }
    state.history_ids.lock().unwrap().retain(|_, v| *v != id);
    save_index_atomic(&app, &index)
}

#[derive(Serialize)]
pub struct ResumeData {
    pub turns: Vec<HistoryTurn>,
    pub model: String,
}

/// Gọi 1 LẦN DUY NHẤT lúc cửa sổ "Kết quả AI" vừa mở do resume từ Lịch sử
/// (xem `history_resume` bên dưới) — trả về turns/model đã ghim sẵn cho đúng
/// window_label đó rồi XOÁ LUÔN khỏi `resume_pending` (dùng 1 lần). Trả về
/// `None` cho MỌI cửa sổ "Kết quả AI" bình thường khác (không phải resume) —
/// frontend coi `None` là "không phải phiên resume, chạy luồng cũ như thường".
#[tauri::command]
pub fn get_resume_data(state: State<'_, AppState>, window_label: String) -> Option<ResumeData> {
    state
        .resume_pending
        .lock()
        .unwrap()
        .remove(&window_label)
        .map(|(turns, model)| ResumeData { turns, model })
}

/// "Tiếp tục hội thoại" từ Lịch sử — mở 1 cửa sổ "Kết quả AI" MỚI, nạp lại
/// đúng ảnh/video gốc (như vừa chụp xong) + toàn bộ turns cũ, rồi các lượt hỏi
/// tiếp SAU ĐÓ cập nhật lại ĐÚNG bản ghi lịch sử này (không tạo bản ghi mới) —
/// ghim sẵn `history_ids[window_label] = id` NGAY TỪ ĐẦU, tận dụng đúng cơ chế
/// đã có ở `history_save_turn` (xem ở trên: "id đã lưu -> chỉ update turns").
///
/// Khác `open_result_window` (commands.rs): KHÔNG có toạ độ vùng vừa chọn để
/// định vị cửa sổ theo (người dùng đang ở cửa sổ Lịch sử, không phải overlay)
/// — đặt cửa sổ giữa màn hình chính thay vì cạnh vùng chụp.
///
/// `async fn` — BẮT BUỘC, cùng lý do với `trigger_capture`/`crop_and_open_result`
/// (commands.rs): lệnh này tạo cửa sổ mới (`WebviewWindowBuilder::build()`).
/// Gọi tạo cửa sổ TRỰC TIẾP trong 1 command ĐỒNG BỘ được dispatch từ luồng IPC
/// tự-deadlock trên Windows/WebView2 (lệnh chờ main thread xử lý việc tạo cửa
/// sổ, trong khi chính main thread đang bị command này chiếm dụng) — bug thực
/// tế đã gặp: bấm "Tiếp tục hội thoại" bị treo loading vô thời hạn, y hệt lỗi
/// từng gặp ở `trigger_capture` trước khi đánh dấu `async`.
#[tauri::command]
pub async fn history_resume(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let (kind, model, turns, media_file) = {
        let index = state.history_index.lock().unwrap();
        let item = index.iter().find(|it| it.id == id).ok_or("Không tìm thấy mục lịch sử này (có thể đã bị xoá)")?;
        (item.kind.clone(), item.model.clone(), item.turns.clone(), item.media_file.clone())
    };

    if kind == "live" {
        return Err("Cuộc gọi trực tiếp chỉ xem lại được, không tiếp tục được.".into());
    }
    let text_only = kind == "chat";

    let session_id = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    let (prefix, media_kind) = match kind.as_str() {
        "video" => (crate::commands::RECORD_LABEL_PREFIX, MediaKind::Video),
        "audio" => (crate::commands::AUDIO_LABEL_PREFIX, MediaKind::Audio),
        _ => (crate::commands::RESULT_LABEL_PREFIX, MediaKind::Image),
    };
    let window_label = format!("{prefix}{session_id}");
    if !text_only {
        let dir = media_dir(&app)?;
        let bytes = fs::read(dir.join(&media_file))
            .map_err(|_| "Ảnh/video gốc của mục này đã bị xoá, không thể tiếp tục hội thoại".to_string())?;
        state
            .media_sessions
            .lock()
            .unwrap()
            .insert(window_label.clone(), vec![MediaItem { bytes, kind: media_kind }]);
    }
    // Ghim NGAY từ đầu -> lượt hỏi tiếp đầu tiên trong cửa sổ này (qua
    // history_save_turn) sẽ thấy "existing_id" và chỉ update turns, không tạo
    // bản ghi lịch sử mới trùng lặp.
    state.history_ids.lock().unwrap().insert(window_label.clone(), id);
    state.resume_pending.lock().unwrap().insert(window_label.clone(), (turns, model));

    let scale = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
    let (mon_x, mon_y, mon_w, mon_h) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let pos = m.position();
            let size = m.size();
            (pos.x, pos.y, size.width, size.height)
        })
        .unwrap_or((0, 0, 1920, 1080));

    let win_w = (480.0_f64 * scale).round();
    let win_h = (340.0_f64 * scale).round();
    let pos_x = (mon_x as f64 + (mon_w as f64 - win_w) / 2.0).max(mon_x as f64);
    let pos_y = (mon_y as f64 + (mon_h as f64 - win_h) / 2.0).max(mon_y as f64);

    let url = if text_only { "result?mode=chat" } else { "result" };
    let win = WebviewWindowBuilder::new(&app, &window_label, WebviewUrl::App(url.into()))
        .title("Kết quả AI")
        .decorations(true)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Không mở được cửa sổ kết quả: {e}"))?;
    let _ = win.set_size(PhysicalSize::new(win_w, win_h));
    let _ = win.set_min_size(Some(PhysicalSize::new(360.0 * scale, 280.0 * scale)));
    let _ = win.set_position(PhysicalPosition::new(pos_x, pos_y));
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

/// Lưu 1 cuộc gọi giọng nói trực tiếp (live.rs gọi khi phiên kết thúc) — chỉ
/// gồm lời thoại 2 phía, không có media (âm thanh cuộc gọi không được ghi
/// lại). Không có lượt nói nào thì không lưu (mở cửa sổ rồi tắt ngay).
pub fn save_live_session(app: &AppHandle, model: &str, turns: Vec<HistoryTurn>, duration_secs: u64) {
    if turns.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    let mut index = state.history_index.lock().unwrap();
    index.insert(
        0,
        HistoryItem {
            id: crate::record::uuid_like(),
            kind: "live".into(),
            created_at: now_ms(),
            media_file: String::new(),
            model: model.to_string(),
            turns,
            duration_secs,
        },
    );
    prune(app, &mut index);
    if let Err(e) = save_index_atomic(app, &index) {
        eprintln!("[snip-ai][history] Không lưu được cuộc gọi live: {e}");
    }
}

/// Xoá SẠCH toàn bộ lịch sử — nút "dọn nhanh" cho máy dùng chung (phòng máy
/// trường học/văn phòng), không giấu trong Cài đặt.
#[tauri::command]
pub fn history_clear_all(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dir = media_dir(&app)?;
    let mut index = state.history_index.lock().unwrap();
    for it in index.iter() {
        remove_media(&dir, &it.media_file);
    }
    index.clear();
    state.history_ids.lock().unwrap().clear();
    save_index_atomic(&app, &index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_index_without_duration_still_loads() {
        let old = r#"[{"id":"a","kind":"image","created_at":1,"media_file":"a.png","model":"m","turns":[]}]"#;
        let items: Vec<HistoryItem> = serde_json::from_str(old).unwrap();
        assert_eq!(items[0].duration_secs, 0);
    }

    /// Mục "chat"/"live" không có media: tên file rỗng KHÔNG được trỏ vào
    /// chính thư mục media (kích thước thư mục sẽ bị tính vào dung lượng, và
    /// remove_file trên thư mục lỗi).
    #[test]
    fn empty_media_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("snap-ai-hist-test-{}", crate::record::uuid_like()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("keep.txt"), b"12345").unwrap();
        assert_eq!(media_len(&dir, ""), 0);
        assert_eq!(media_len(&dir, "keep.txt"), 5);
        remove_media(&dir, "");
        assert!(dir.join("keep.txt").exists() && dir.exists());
        remove_media(&dir, "keep.txt");
        assert!(!dir.join("keep.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
