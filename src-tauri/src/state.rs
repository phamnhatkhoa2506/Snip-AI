use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};
use tauri_plugin_global_shortcut::Shortcut;

/// Phím tắt chụp màn hình đang hoạt động — tách khỏi `AppState` vì `Shortcut`
/// không implement `Default`, còn `AppState` derive `Default`. Được khởi tạo
/// thủ công trong `setup()` (lib.rs) sau khi đọc giá trị đã lưu trên đĩa (hoặc
/// mặc định), rồi `.manage()` riêng.
pub struct HotkeyState {
    pub current: Mutex<Shortcut>,
}

/// Phím tắt QUAY VIDEO — tách hẳn khỏi `HotkeyState` (phím tắt chụp ảnh), 2
/// tổ hợp hoàn toàn độc lập, đổi cái này không ảnh hưởng cái kia. Xem record.rs.
pub struct RecordHotkeyState {
    pub current: Mutex<Shortcut>,
}

/// Phím tắt SNAP AUDIO — độc lập với 2 phím tắt trên. Xem audio_snap.rs.
pub struct AudioHotkeyState {
    pub current: Mutex<Shortcut>,
}

/// Phiên Snap Audio đang thu (chỉ 1 phiên tại 1 thời điểm) — xem audio_snap.rs.
pub struct AudioSnapSession {
    pub stop: Arc<AtomicBool>,
    pub discard: Arc<AtomicBool>,
}

/// 1 `reqwest::Client` DÙNG CHUNG cho mọi lệnh gọi AI, thay vì tạo mới mỗi lần
/// gọi API. `reqwest::Client` giữ pool kết nối HTTP/TLS bên trong — tạo `Client`
/// mới nghĩa là bắt tay TLS (TLS handshake) lại từ đầu mỗi request, cộng thêm
/// ~100-300ms độ trễ oan uổng trước khi request thật sự được gửi đi. Dùng
/// chung 1 client, các request tới CÙNG 1 host (VD nhiều lượt hỏi liên tiếp
/// trong 1 cuộc hội thoại) sẽ tái dùng kết nối đã mở sẵn.
pub struct HttpClientState {
    pub client: reqwest::Client,
}

impl Default for HttpClientState {
    fn default() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

/// State dùng chung toàn app, quản lý qua Tauri managed state (thread-safe qua Mutex).
///
/// `screenshot_png`/`monitor_bounds`/`scale_factor` là DÙNG CHUNG (singleton)
/// — chỉ có 1 phiên "đang chọn vùng" tại 1 thời điểm là hợp lý (1 chuột không
/// chọn được 2 vùng cùng lúc), nên overlay không cần multi-instance.
///
/// `crop_sessions` thì NGƯỢC LẠI — mỗi lần crop xong mở 1 cửa sổ "Kết quả AI"
/// ĐỘC LẬP (label riêng), nên ảnh phải lưu theo từng phiên (key = label cửa
/// sổ đó) thay vì 1 slot chung — trước đây dùng 1 slot chung khiến snip lần 2
/// trong lúc popup lần 1 còn mở sẽ đóng mất popup lần 1.
#[derive(Default)]
pub struct AppState {
    pub screenshot_png: Mutex<Option<Vec<u8>>>,
    /// Toạ độ + kích thước của monitor đã chụp, dùng để định vị cửa sổ overlay
    /// và tính vị trí đặt cửa sổ kết quả cho đúng.
    pub monitor_bounds: Mutex<Option<MonitorBounds>>,
    /// Scale factor (DPI) của monitor đã chụp — TẤT CẢ toạ độ/kích thước cửa sổ
    /// trong app này đều dùng đơn vị PHYSICAL pixel (khớp trực tiếp với pixel
    /// ảnh chụp màn hình từ `xcap`), nên khi cần set kích thước UI cố định theo
    /// ý đồ thiết kế (VD cửa sổ kết quả rộng ~480 logical px) phải nhân với
    /// scale_factor này để ra đúng physical px, tránh cửa sổ bị nhỏ/lớn sai
    /// trên màn hình DPI != 100%.
    pub scale_factor: Mutex<f64>,

    /// Chuỗi ảnh/video của từng phiên (key = label cửa sổ "Kết quả AI" của
    /// phiên đó, VD "result-3"/"record-5"), ĐÚNG THỨ TỰ đã chụp/quay — ảnh và
    /// video ĐAN XEN tự do trong cùng 1 chuỗi (VD chụp 1 ảnh, quay thêm 1
    /// video, chụp tiếp 1 ảnh), mỗi phần tử tự mang loại của nó (`MediaKind`).
    ///
    /// Trước đây tách 2 map riêng (`crop_sessions` cho ảnh, `video_sessions`
    /// cho video) với giả định "1 phiên chỉ toàn 1 loại" — không trộn được, và
    /// nếu lỡ nối video vào phiên ảnh thì AI chỉ thấy phần ảnh (map ảnh được
    /// đọc trước, map video bị bỏ qua hoàn toàn). Gộp lại 1 chuỗi có đánh dấu
    /// loại để giữ đúng thứ tự thật giữa ảnh và video.
    ///
    /// Tiền tố label cửa sổ ("result-"/"record-") giờ CHỈ cho biết phần tử
    /// ĐẦU TIÊN của chuỗi là ảnh hay video — không còn quyết định loại của cả
    /// phiên. Dọn dẹp cả entry khi cửa sổ đó đóng (xem `on_window_event` trong
    /// lib.rs) — tránh rò rỉ bộ nhớ khi dùng app lâu, snip nhiều lần.
    pub media_sessions: Mutex<HashMap<String, Vec<MediaItem>>>,
    /// Bộ đếm tăng dần để sinh label cửa sổ "Kết quả AI" không trùng nhau.
    pub next_session_id: AtomicU32,

    /// Đánh dấu cửa sổ Settings ("main") có đang bị ẩn TẠM để nhường chỗ cho
    /// việc chọn vùng hay không — chỉ true trong khoảng từ lúc bấm hotkey tới
    /// lúc chọn xong/huỷ. Dùng để quyết định có nên hiện lại nó hay không khi
    /// overlay đóng (nếu người dùng tự ẩn Settings từ trước, không nên tự ý
    /// hiện lại).
    pub main_hidden_for_snip: Mutex<bool>,

    /// Cờ báo dừng của phiên quay đang chạy (nếu có) — `stop_recording` set
    /// cờ này thành `true` để dừng sớm trước mốc 30s tự động. `None` nghĩa là
    /// không có phiên quay nào đang chạy.
    pub recording_stop_flag: Mutex<Option<Arc<AtomicBool>>>,
    /// Cờ HUỶ (khác dừng) — bấm nút "X" trên thanh công cụ nổi lúc đang quay
    /// sẽ set cờ này TRƯỚC KHI set `recording_stop_flag`, để khi quay dừng lại
    /// biết là huỷ bỏ (không mở cửa sổ "Kết quả AI", không giữ video) thay vì
    /// dừng bình thường (mở cửa sổ kết quả như thường lệ).
    pub recording_discard: Mutex<bool>,
    /// Thông tin cần để mở cửa sổ "Kết quả AI" SAU KHI quay xong — lưu tạm từ
    /// lúc bắt đầu quay (`start_region_recording`), tiêu thụ 1 lần khi phiên
    /// quay kết thúc (xem record.rs). Khác luồng ảnh: ảnh mở cửa sổ kết quả
    /// NGAY (rồi nạp ảnh sau), video phải đợi quay xong mới có gì để hiện nên
    /// cửa sổ kết quả chỉ mở ở bước cuối.
    pub recording_pending: Mutex<Option<PendingRecordResult>>,

    /// Lịch sử ảnh/video + hội thoại — nạp 1 lần từ đĩa lúc app khởi động
    /// (xem history.rs::init), sau đó mọi lệnh history_* đọc/ghi thẳng trên
    /// bản trong RAM này, không parse lại index.json mỗi lần gọi.
    pub history_index: Mutex<Vec<crate::history::HistoryItem>>,
    /// window_label -> id bản ghi lịch sử TƯƠNG Ứng, chỉ tồn tại trong lúc
    /// cửa sổ "Kết quả AI" của phiên đó còn mở (dọn khi cửa sổ đóng, xem
    /// on_window_event trong lib.rs) — để các lượt "hỏi tiếp" trong CÙNG 1
    /// cửa sổ cập nhật lại đúng 1 bản ghi thay vì tạo bản ghi mới mỗi câu hỏi.
    pub history_ids: Mutex<HashMap<String, String>>,

    /// Tài liệu đính kèm (ảnh/PDF) NGOÀI ảnh/video chính đã chụp — GIAI ĐOẠN 1
    /// của tính năng "đính kèm file" (xem attachments.rs), phục vụ trường hợp
    /// người dùng chụp 1 vùng nhỏ nhưng muốn đưa thêm tài liệu gốc (PDF nhiều
    /// trang, ảnh chụp khác...) làm ngữ cảnh. Dọn dẹp khi cửa sổ đóng, cùng
    /// chỗ với crop_sessions/video_sessions (xem on_window_event trong lib.rs).
    pub attachment_sessions: Mutex<HashMap<String, Vec<AttachmentEntry>>>,

    /// Dữ liệu tạm để cửa sổ "Kết quả AI" MỚI MỞ nạp thẳng vào chế độ "đang
    /// chat" khi người dùng bấm "Tiếp tục hội thoại" từ Lịch sử — key = label
    /// cửa sổ vừa tạo cho phiên resume đó. Frontend gọi `get_resume_data` đúng
    /// 1 lần lúc mount rồi entry bị lấy ra luôn (dùng 1 lần, không cần dọn
    /// riêng ở on_window_event vì đã tự rỗng ngay khi đọc).
    pub resume_pending: Mutex<HashMap<String, (Vec<crate::history::HistoryTurn>, String)>>,

    /// Phiên Snap Audio đang thu, nếu có — xem audio_snap.rs.
    pub audio_snap: Mutex<Option<AudioSnapSession>>,

    /// Phiên quay video đã THẬT SỰ bắt đầu ghi (encoder khởi động xong, có
    /// thể mất vài giây) — thanh công cụ hỏi lại lúc mount phòng trường hợp
    /// sự kiện "recording:started" bắn ra trước khi nó kịp lắng nghe.
    pub recording_live: AtomicBool,

    /// Phiên trò chuyện trực tiếp đang chạy, nếu có — xem live.rs.
    pub live: Mutex<Option<crate::live::LiveSession>>,
    /// Phiên phụ đề trực tiếp đang chạy, nếu có — xem caption.rs.
    pub caption: Mutex<Option<crate::caption::CaptionSession>>,
}

/// Trần số lượng media (ảnh HOẶC video) cho phép gom vào CÙNG 1 chuỗi/phiên —
/// tránh chuỗi dài vô hạn làm payload gọi AI phình to, chậm và tốn quota vô
/// tội vạ. Xem "Chụp thêm bước" (append_capture_to_session/start_region_recording).
pub const MAX_CHAIN_ITEMS: usize = 8;

/// Loại 1 phần tử trong chuỗi media của phiên — xem `AppState::media_sessions`.
/// Serialize thành "image"/"video" cho frontend tự chọn thẻ <img>/<video>.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    /// Snap Audio (audio_snap.rs) — WAV 16kHz mono.
    Audio,
}

impl MediaKind {
    pub fn mime(self) -> &'static str {
        match self {
            MediaKind::Image => "image/png",
            MediaKind::Video => "video/mp4",
            MediaKind::Audio => "audio/wav",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MediaItem {
    pub bytes: Vec<u8>,
    pub kind: MediaKind,
}

/// 1 file đính kèm (ảnh/PDF, xem attachments.rs) — GIỮ NGUYÊN `bytes` gốc dù
/// đã upload qua Gemini File API (không phải mọi lượt hỏi đều đi qua File
/// API nếu người dùng đổi ý/xoá rồi thêm lại), `file_uri` chỉ là CACHE của
/// lần upload gần nhất còn dùng được (~48h, xem file_api.rs) — tránh upload
/// lại y hệt file đó mỗi lần hỏi thêm trong cùng phiên.
#[derive(Clone, Debug)]
pub struct AttachmentEntry {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub name: String,
    pub file_uri: Option<String>,
    /// Key backend đã upload file này (xem file_api::UploadedFile) — `file_uri`
    /// chỉ dùng được với đúng key đó.
    pub key_id: Option<String>,
}

/// Nối 1 ảnh/video vào CUỐI chuỗi của phiên `window_label` — dùng chung cho
/// cả "Chụp thêm ảnh" (commands.rs) lẫn "Quay thêm video" (record.rs), bất
/// kể phiên bắt đầu bằng ảnh hay video. Vượt `MAX_CHAIN_ITEMS` thì bỏ bớt
/// phần tử CŨ NHẤT ("trượt cửa sổ") thay vì chặn hẳn — dễ hiểu với người
/// dùng hơn là 1 lỗi "đã đầy".
pub fn push_media(state: &AppState, window_label: &str, item: MediaItem) {
    let mut sessions = state.media_sessions.lock().unwrap();
    let list = sessions.entry(window_label.to_string()).or_default();
    list.push(item);
    while list.len() > MAX_CHAIN_ITEMS {
        list.remove(0);
    }
}

#[derive(Clone, Debug)]
pub struct PendingRecordResult {
    pub monitor: MonitorBounds,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub session_id: u32,
    /// `Some(label)` = quay THÊM vào phiên đang mở (label cửa sổ đó), KHÔNG
    /// mở cửa sổ "Kết quả AI" mới khi quay xong — chỉ push video vào đúng
    /// `video_sessions[label]` rồi báo cho cửa sổ đó tự nạp lại. `None` = hành
    /// vi cũ (quay xong mở cửa sổ kết quả mới).
    pub append_to: Option<String>,
    /// Cờ "Tự động chép vào clipboard" đọc từ settings.ts lúc BẮT ĐẦU quay
    /// (không đọc lại lúc quay XONG — nhất quán với hành vi lúc người dùng
    /// bấm nút, dù có đổi cài đặt giữa chừng lúc đang quay). Xem
    /// clipboard_copy.rs::copy_file_to_clipboard.
    pub auto_copy: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
