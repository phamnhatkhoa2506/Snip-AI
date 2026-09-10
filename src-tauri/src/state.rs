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

    /// Ảnh PNG đã crop, theo từng phiên (key = label cửa sổ "Kết quả AI" của
    /// phiên đó, VD "result-3"). Dọn dẹp entry tương ứng khi cửa sổ đó đóng
    /// (xem `on_window_event` trong lib.rs) — tránh rò rỉ bộ nhớ khi dùng app
    /// lâu, snip nhiều lần.
    pub crop_sessions: Mutex<HashMap<String, Vec<u8>>>,
    /// Bộ đếm tăng dần để sinh label cửa sổ "Kết quả AI" không trùng nhau.
    pub next_session_id: AtomicU32,

    /// Đánh dấu cửa sổ Settings ("main") có đang bị ẩn TẠM để nhường chỗ cho
    /// việc chọn vùng hay không — chỉ true trong khoảng từ lúc bấm hotkey tới
    /// lúc chọn xong/huỷ. Dùng để quyết định có nên hiện lại nó hay không khi
    /// overlay đóng (nếu người dùng tự ẩn Settings từ trước, không nên tự ý
    /// hiện lại).
    pub main_hidden_for_snip: Mutex<bool>,

    /// Video MP4 đã quay xong, theo từng phiên (key = label cửa sổ "Kết quả
    /// AI" của phiên đó) — cùng cơ chế với `crop_sessions` nhưng cho video.
    pub video_sessions: Mutex<HashMap<String, Vec<u8>>>,
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
}

#[derive(Clone, Copy, Debug)]
pub struct PendingRecordResult {
    pub monitor: MonitorBounds,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub session_id: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
