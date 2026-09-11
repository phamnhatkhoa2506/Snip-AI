//! Quay màn hình + gửi cho AI (Gemini hỗ trợ input video) — tận dụng ý tưởng
//! "Record" của Windows Snipping Tool nhưng thay vì chỉ lưu file, gửi thẳng
//! cho AI phân tích giống luồng snip ảnh.
//!
//! Dùng crate `windows-capture` (Windows Graphics Capture API thuần, KHÔNG
//! phụ thuộc ffmpeg) — crate này tự lo cả việc capture từng khung hình LẪN
//! encode ra MP4 (H264, tăng tốc phần cứng qua Media Foundation), nên module
//! này chỉ cần điều phối start/stop, không phải tự viết pipeline COM/MFT.
//!
//! Giới hạn 30 giây/lần quay — vừa đủ cho việc demo 1 lỗi/1 thao tác, vừa
//! kiểm soát được chi phí token (video tốn ~100-300 token/giây ở phía Gemini,
//! nặng hơn ảnh tĩnh nhiều — quan trọng vì app dùng chung pool API key
//! free-tier giữa nhiều người dùng, xem backend/).
//!
//! QUAY THEO VÙNG (không phải luôn cả màn hình): Windows Graphics Capture
//! API chỉ capture được nguyên 1 monitor/window, KHÔNG cắt theo vùng tuỳ ý
//! ngay từ API gốc — nên vẫn capture nguyên monitor, nhưng CẮT TỪNG KHUNG
//! HÌNH (`Frame::buffer_crop`) về đúng vùng người dùng chọn trước khi đưa cho
//! encoder, và cấu hình encoder với kích thước ĐÃ CẮT (không phải kích thước
//! monitor) — kết quả là video xuất ra chỉ chứa đúng vùng đã chọn.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::commands::{self, RECORD_LABEL_PREFIX};
use crate::state::AppState;

/// Giới hạn thời lượng quay tối đa — xem giải thích lý do ở đầu file.
const MAX_RECORD_SECONDS: u64 = 30;

const TOOLBAR_LABEL: &str = "record-toolbar";

/// Thông số encode — cố tình ĐẶT THẤP hơn nhiều so với mặc định của crate
/// (15 Mbps @ 60fps). Người xem clip này là AI chứ không phải người: Gemini
/// lấy mẫu video ở khoảng 1 khung/giây, nên mọi bit dôi ra cho 60fps/bitrate
/// cao đều vô ích, chỉ làm file phình và upload lâu. Đo thực tế với mặc định:
/// clip 30s ra file 168 MB — VƯỢT trần 100 MB cho video gửi inline của Gemini,
/// tức là tính năng gãy ngay ở ca dùng dài nhất. Với 2.5 Mbps, 30s ≈ 9 MB mà
/// chữ trên màn hình vẫn đọc rõ.
const ENCODE_BITRATE_BPS: u32 = 2_500_000;
const ENCODE_FPS: u32 = 30;

/// Khoảng cách tối thiểu giữa 2 khung hình được gửi vào encoder. WGC có thể
/// bắn frame theo tần số quét màn hình (60/144Hz); gửi hết vào encoder đã khai
/// báo 30fps vừa lệch khai báo vừa tốn CPU gấp đôi cho vòng cắt vùng.
const MIN_FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / ENCODE_FPS as u64);

/// Cờ báo dừng, kiểm tra ở mỗi khung hình nhận được — cách duy nhất để dừng
/// việc quay từ BÊN NGOÀI closure của `windows-capture` (capture chạy trên
/// thread riêng, đồng bộ/blocking cho tới khi tự dừng).
type StopFlag = Arc<AtomicBool>;

/// Vùng cần cắt (x, y, width, height) — toạ độ TƯƠNG ĐỐI trong khung hình gốc
/// (không cộng offset monitor, giống cách `capture::crop_png` xử lý ảnh).
/// `None` = quay nguyên màn hình, không cắt gì (đường cũ, vẫn giữ để lỡ cần).
type CropRegion = Option<(u32, u32, u32, u32)>;

struct Capture {
    encoder: Option<VideoEncoder>,
    start: Instant,
    crop: CropRegion,
    /// Mốc của khung hình gần nhất ĐÃ gửi vào encoder — dùng để bỏ bớt khung
    /// khi WGC bắn nhanh hơn `ENCODE_FPS`.
    last_sent: Option<Duration>,
}

impl Capture {
    /// Chốt file MP4 (flush + ghi index/moov) — gọi TỪ BÊN NGOÀI, SAU KHI
    /// luồng capture đã dừng hẳn (không còn khung hình nào được gửi vào nữa).
    /// Trước đây việc này nằm trong `on_frame_arrived`, chính là gốc rễ của
    /// bug "quay mãi không dừng" — xem giải thích ở `run_capture_blocking`.
    fn finalize(&mut self) -> Result<(), String> {
        match self.encoder.take() {
            Some(encoder) => encoder.finish().map_err(|e| format!("Lỗi kết thúc encode video: {e}")),
            None => Ok(()),
        }
    }
}

impl GraphicsCaptureApiHandler for Capture {
    /// (rộng, cao MÀ ENCODER SẼ DÙNG — đã tính sẵn theo crop nếu có, đường
    /// dẫn file MP4 đích, vùng cắt)
    type Flags = (u32, u32, PathBuf, CropRegion);
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (encode_width, encode_height, out_path, crop) = ctx.flags;
        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(encode_width, encode_height)
                .bitrate(ENCODE_BITRATE_BPS)
                .frame_rate(ENCODE_FPS),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            out_path,
        )?;
        Ok(Self { encoder: Some(encoder), start: Instant::now(), crop, last_sent: None })
    }

    /// CHỈ encode khung hình — KHÔNG còn kiểm tra cờ dừng/mốc 30s ở đây nữa.
    /// Lý do (bug thực tế): callback này chỉ chạy KHI CÓ KHUNG HÌNH MỚI. Nếu
    /// vùng đang quay đứng yên (không có gì thay đổi trên màn hình), WGC
    /// không gửi frame nào → callback không chạy → cờ dừng/mốc 30s không bao
    /// giờ được kiểm tra → bấm Dừng vô tác dụng, quá 30s vẫn quay. Việc dừng
    /// giờ do 1 luồng canh riêng lo (xem `run_capture_blocking`).
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Bỏ khung hình đến sớm hơn nhịp `ENCODE_FPS` — xem `MIN_FRAME_INTERVAL`.
        let now = self.start.elapsed();
        if let Some(prev) = self.last_sent {
            if now.saturating_sub(prev) < MIN_FRAME_INTERVAL {
                return Ok(());
            }
        }
        self.last_sent = Some(now);

        if let Some(encoder) = self.encoder.as_mut() {
            match self.crop {
                None => {
                    // Đường cũ — nguyên khung hình, dùng API cấp cao của crate
                    // (không phải tự tay lấy buffer).
                    encoder.send_frame(frame)?;
                }
                Some((x, y, w, h)) => {
                    // Cắt đúng vùng đã chọn trước khi đưa cho encoder — xem
                    // giải thích ở đầu file vì sao phải làm vậy (WGC không hỗ
                    // trợ cắt vùng ngay từ API capture).
                    //
                    // CỐ Ý dùng `frame.buffer()` (lấy NGUYÊN khung hình, API
                    // đơn giản nhất của crate) rồi tự cắt bằng tay bằng cách
                    // copy từng dòng — KHÔNG dùng `frame.buffer_crop()` (API
                    // "cắt sẵn" chuyên biệt của crate): đã gặp thực tế app bị
                    // đơ hoàn toàn (kể cả nút Dừng cũng vô tác dụng, dấu hiệu
                    // luồng quay bị kẹt trong 1 lời gọi native không bao giờ
                    // trả về) khi dùng `buffer_crop`. Cách tự cắt tay chỉ dùng
                    // API đọc buffer thô đơn giản, rủi ro thấp hơn nhiều.
                    let mut full = frame.buffer()?;
                    let frame_w = full.width();
                    let frame_h = full.height();
                    let row_pitch = full.row_pitch() as usize;
                    const BPP: usize = 4; // BGRA8 — mỗi pixel 4 byte

                    // Kẹp toạ độ trong biên khung hình thật sự nhận được — toạ
                    // độ frontend tính có thể lệch vài px so với kích thước
                    // WGC capture thật sự trả về (viền/DPI...), lệch mà không
                    // kẹp có thể đọc tràn bộ đệm.
                    let cx = x.min(frame_w.saturating_sub(1));
                    let cy = y.min(frame_h.saturating_sub(1));
                    let cw = w.min(frame_w.saturating_sub(cx)).max(1);
                    let ch = h.min(frame_h.saturating_sub(cy)).max(1);

                    let raw = full.as_raw_buffer();
                    let mut packed = Vec::with_capacity(cw as usize * BPP * ch as usize);
                    // Ghi các dòng theo thứ tự NGƯỢC (dưới lên trên) — tài liệu
                    // của `send_frame_buffer` ghi rõ Windows chờ dữ liệu dạng
                    // BGRA + bottom-to-top ở đường này, còn buffer đọc từ
                    // texture là top-down; không đảo thì video bị lộn ngược.
                    for row in (0..ch).rev() {
                        let start = (cy + row) as usize * row_pitch + cx as usize * BPP;
                        let end = start + cw as usize * BPP;
                        if end <= raw.len() {
                            packed.extend_from_slice(&raw[start..end]);
                        }
                    }

                    // Windows `TimeSpan` tính bằng tick 100ns, KHÔNG phải
                    // nanosecond — chia 100, nếu không mọi mốc thời gian bị
                    // thổi lên gấp 10 lần (video dài/chậm sai hẳn).
                    let timestamp_ticks = (now.as_nanos() / 100) as i64;
                    encoder.send_frame_buffer(&packed, timestamp_ticks)?;
                }
            }
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Chạy toàn bộ phiên quay — BLOCKING cho tới khi quay xong, nên PHẢI gọi
/// trong 1 thread riêng (`std::thread::spawn`), không gọi trực tiếp trong
/// async runtime của Tokio.
///
/// KIẾN TRÚC DỪNG (điểm mấu chốt, đã sửa sau bug "quay mãi không dừng"):
/// KHÔNG dùng `Capture::start()` + kiểm tra cờ dừng trong `on_frame_arrived`
/// nữa — callback đó chỉ chạy KHI CÓ KHUNG HÌNH MỚI, mà WGC không gửi khung
/// hình nào khi vùng quay đứng yên, nên cờ dừng/mốc 30s không bao giờ được
/// kiểm tra (bấm Dừng vô tác dụng, quá 30s vẫn quay).
///
/// Thay vào đó: `start_free_threaded()` cho capture chạy trên luồng riêng của
/// nó, còn luồng này làm "canh gác" — cứ 100ms kiểm tra cờ dừng/mốc thời gian
/// 1 lần (hoàn toàn độc lập với việc có khung hình hay không), rồi gọi
/// `CaptureControl::stop()` — hàm này gửi `WM_QUIT` thẳng cho luồng capture
/// nên dừng được kể cả khi không có khung hình nào tới.
fn run_capture_blocking(out_path: PathBuf, stop_flag: StopFlag, crop: CropRegion) -> Result<(), String> {
    let monitor = Monitor::primary().map_err(|e| format!("Không tìm được màn hình chính: {e}"))?;

    // BẮT BUỘC làm tròn kích thước vùng quay xuống số CHẴN — H.264 dùng chroma
    // 4:2:0 nên chiều rộng/cao lẻ là không hợp lệ. Bug thực tế đã gặp: chọn
    // vùng 851x782 (rộng lẻ) khiến encoder Media Foundation không khởi tạo
    // được, luồng transcode không bao giờ kết thúc, và `encoder.finish()` treo
    // vĩnh viễn -> bấm Dừng như không có tác dụng. Quay nguyên màn hình không
    // dính lỗi này chỉ vì độ phân giải màn hình luôn chẵn (VD 1920x1080).
    let crop = crop.map(|(x, y, w, h)| (x, y, (w & !1).max(2), (h & !1).max(2)));

    // Kích thước ENCODER dùng — nếu có crop thì dùng đúng kích thước vùng đã
    // chọn (video xuất ra chỉ chứa vùng đó), không thì dùng nguyên màn hình.
    let (encode_w, encode_h) = match crop {
        Some((_, _, w, h)) => (w, h),
        None => (
            monitor.width().map_err(|e| format!("Không đọc được chiều rộng màn hình: {e}"))?,
            monitor.height().map_err(|e| format!("Không đọc được chiều cao màn hình: {e}"))?,
        ),
    };

    // Màu RGBA8 cho đường quay-nguyên-màn-hình (dùng `send_frame` cấp cao,
    // crate tự lo định dạng) — BGRA8 cho đường crop-vùng (bắt buộc, vì
    // `send_frame_buffer` — API cấp thấp hơn — yêu cầu đúng định dạng BGRA8).
    let color_format = if crop.is_some() { ColorFormat::Bgra8 } else { ColorFormat::Rgba8 };

    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        color_format,
        (encode_w, encode_h, out_path, crop),
    );

    let control = Capture::start_free_threaded(settings).map_err(|e| format!("Lỗi bắt đầu quay màn hình: {e}"))?;
    // Lấy tay cầm tới struct handler TRƯỚC khi `stop()` (hàm đó "ăn" luôn
    // `control`) — cần nó để chốt file MP4 sau khi capture đã dừng hẳn.
    let handler = control.callback();

    let started = Instant::now();
    loop {
        if stop_flag.load(Ordering::SeqCst) {
            eprintln!("[snip-ai] Nhận lệnh dừng quay từ người dùng.");
            break;
        }
        if started.elapsed() >= Duration::from_secs(MAX_RECORD_SECONDS) {
            eprintln!("[snip-ai] Đã quay đủ {MAX_RECORD_SECONDS}s, tự dừng.");
            break;
        }
        if control.is_finished() {
            eprintln!("[snip-ai] Luồng quay tự kết thúc (có thể do lỗi).");
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Chốt file MP4 TRƯỚC KHI dừng luồng capture — thứ tự này RẤT QUAN TRỌNG
    // (bug thực tế: làm ngược lại thì treo vĩnh viễn ngay sau khi capture
    // dừng, thanh công cụ không bao giờ đóng). Lý do: `VideoEncoder` bên dưới
    // là đối tượng COM/Media Foundation được TẠO TRÊN luồng capture. Nếu gọi
    // `finish()` sau khi `control.stop()` đã kết liễu luồng đó, apartment COM
    // của nó không còn ai bơm message → lời gọi bị marshal vào hư vô, treo
    // không bao giờ trả về. Gọi lúc luồng còn sống thì an toàn ở cả 2 kiểu
    // apartment (MTA gọi thẳng, STA marshal qua message pump vẫn đang chạy).
    //
    // An toàn về tranh chấp: `handler` là mutex bao chính struct handler mà
    // luồng capture phải khoá mỗi lần `on_frame_arrived` — nên `finalize()`
    // không bao giờ chạy song song với việc encode 1 khung hình. Sau khi chốt,
    // `encoder` thành `None`, các khung hình đến sau tự bỏ qua (xem
    // `on_frame_arrived`), không lỗi gì.
    //
    // `handler` là `parking_lot::Mutex` (của crate windows-capture), không
    // phải `std::sync::Mutex` — `lock()` trả thẳng guard, không có Result.
    eprintln!("[snip-ai] Đang chốt file video…");
    handler.lock().finalize()?;
    eprintln!("[snip-ai] Đã chốt file video, đang dừng luồng quay…");

    let stop_result = control.stop();
    eprintln!("[snip-ai] Luồng quay đã dừng hẳn.");

    stop_result.map_err(|e| format!("Lỗi dừng quay màn hình: {e}"))
}

/// Bắt đầu 1 phiên quay mới — trả về NGAY (không đợi quay xong), việc quay
/// chạy nền trên thread riêng. Khi quay xong (đủ 30s, bị dừng sớm, hoặc bị
/// huỷ), thanh công cụ nổi tự đóng và — trừ khi bị HUỶ (xem
/// `AppState::recording_discard`) — cửa sổ "Kết quả AI" mới được mở ra, video
/// nạp sẵn vào đó luôn (khác luồng ảnh: ảnh mở cửa sổ kết quả TRƯỚC rồi mới
/// nạp ảnh sau, video phải đợi quay xong mới có gì để hiện nên làm ngược lại).
///
/// `region`: `Some((x,y,w,h))` để quay đúng vùng đã chọn (đường chính,
/// `start_region_recording` luôn truyền vào), `None` = quay nguyên màn hình
/// (giữ lại cho khả năng dùng sau, hiện không có UI nào gọi kiểu này).
pub async fn start_recording(app: AppHandle, region: CropRegion) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Chỉ 1 phiên quay tại 1 thời điểm — giống lý do overlay chọn vùng là
    // singleton (Windows Graphics Capture API chỉ capture được từ 1 "phiên"
    // cho cùng 1 nguồn tại 1 thời điểm hợp lý theo cách người dùng thao tác).
    {
        let guard = state.recording_stop_flag.lock().unwrap();
        if guard.is_some() {
            return Err("Đang có 1 phiên quay khác chạy rồi.".into());
        }
    }

    let stop_flag: StopFlag = Arc::new(AtomicBool::new(false));
    *state.recording_stop_flag.lock().unwrap() = Some(stop_flag.clone());

    let out_path = std::env::temp_dir().join(format!("snap-ai-recording-{}.mp4", uuid_like()));

    let app_clone = app.clone();
    let out_path_clone = out_path.clone();
    std::thread::spawn(move || {
        let result = run_capture_blocking(out_path_clone.clone(), stop_flag, region);
        let app_for_main_thread = app_clone.clone();

        let state = app_clone.state::<AppState>();
        // Dọn cờ dừng — phiên đã kết thúc (dù thành công hay lỗi), cho phép
        // bắt đầu phiên quay mới.
        *state.recording_stop_flag.lock().unwrap() = None;
        let discard = {
            let mut guard = state.recording_discard.lock().unwrap();
            std::mem::replace(&mut *guard, false)
        };
        let pending = state.recording_pending.lock().unwrap().take();

        // Đóng thanh công cụ + (nếu không bị huỷ) mở cửa sổ "Kết quả AI" —
        // BẮT BUỘC chạy trên main thread vì đụng tới việc tạo/đóng cửa sổ
        // WebView2 (xem giải thích chi tiết ở lib.rs cho lý do tương tự).
        let finish = move || {
            eprintln!("[snip-ai] Đang đóng thanh công cụ quay…");
            if let Some(win) = app_clone.get_webview_window(TOOLBAR_LABEL) {
                let _ = win.close();
            }

            if discard {
                eprintln!("[snip-ai] Phiên quay đã bị huỷ, không giữ video.");
                let _ = std::fs::remove_file(&out_path_clone);
                return;
            }

            match &result {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("[snip-ai] Lỗi quay màn hình: {e}");
                    // Không có cửa sổ nào khác để báo lỗi (thanh công cụ vừa
                    // đóng) — log lại là đủ, người dùng thấy quay không ra gì
                    // thì thử lại là chính.
                    return;
                }
            }

            let Some(pending) = pending else {
                eprintln!("[snip-ai] Quay xong nhưng thiếu thông tin để mở cửa sổ kết quả (bug?).");
                return;
            };

            let bytes = match std::fs::read(&out_path_clone) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[snip-ai] Lỗi đọc file video sau khi quay: {e}");
                    return;
                }
            };
            let _ = std::fs::remove_file(&out_path_clone); // đã đọc vào RAM, xoá file tạm

            let state = app_clone.state::<AppState>();

            // "+ Chụp thêm bước" ở 1 phiên video — PUSH vào chuỗi của cửa sổ
            // ĐANG MỞ đó, không mở cửa sổ mới.
            if let Some(label) = pending.append_to {
                eprintln!("[snip-ai] Quay xong, {} bytes MP4 -> thêm vào chuỗi ({label})", bytes.len());
                {
                    let mut sessions = state.video_sessions.lock().unwrap();
                    let list = sessions.entry(label.clone()).or_default();
                    list.push(bytes);
                    while list.len() > crate::state::MAX_CHAIN_ITEMS {
                        list.remove(0);
                    }
                }
                let _ = app_clone.emit_to(&label, "ai:chain-updated", ());
                return;
            }

            let window_label = format!("{RECORD_LABEL_PREFIX}{}", pending.session_id);
            eprintln!("[snip-ai] Quay xong, {} bytes MP4 -> mở cửa sổ kết quả ({window_label})", bytes.len());

            state.video_sessions.lock().unwrap().insert(window_label.clone(), vec![bytes]);

            if let Err(e) = commands::open_result_window(
                &app_clone,
                pending.monitor,
                &window_label,
                pending.anchor_x,
                pending.anchor_y,
                0,
                pending.session_id,
            ) {
                eprintln!("[snip-ai] Không mở được cửa sổ kết quả sau khi quay: {e}");
            }
        };
        let _ = app_for_main_thread.run_on_main_thread(finish);
    });

    Ok(())
}

#[tauri::command]
pub fn stop_recording(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let guard = state.recording_stop_flag.lock().unwrap();
    match guard.as_ref() {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        }
        None => Err("Không có phiên quay nào đang chạy.".into()),
    }
}

/// 1 phiên có thể có NHIỀU video (chuỗi quay) — trả về video MỚI NHẤT (dùng
/// cho preview chính); muốn cả chuỗi thì dùng `get_recording_chain_base64`.
#[tauri::command]
pub fn get_recording_base64(state: tauri::State<'_, AppState>, window_label: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let sessions = state.video_sessions.lock().unwrap();
    let list = sessions
        .get(&window_label)
        .ok_or("Không tìm thấy video cho phiên này (cửa sổ có thể đã bị đóng)")?;
    let bytes = list.last().ok_or("Phiên này chưa có video nào")?;
    Ok(STANDARD.encode(bytes))
}

/// Toàn bộ chuỗi video đã quay cho phiên này, ĐÚNG THỨ TỰ — xem
/// `get_crop_chain_base64` (tương đương cho ảnh) để hiểu ngữ cảnh dùng.
#[tauri::command]
pub fn get_recording_chain_base64(state: tauri::State<'_, AppState>, window_label: String) -> Result<Vec<String>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let sessions = state.video_sessions.lock().unwrap();
    let list = sessions
        .get(&window_label)
        .ok_or("Không tìm thấy video cho phiên này (cửa sổ có thể đã bị đóng)")?;
    Ok(list.iter().map(|b| STANDARD.encode(b)).collect())
}

/// ID ngẫu nhiên đủ dùng để đặt tên file tạm không trùng nhau — không cần cả
/// crate `uuid` chỉ để làm việc này.
// `pub(crate)` — dùng lại ở history.rs để sinh id bản ghi lịch sử, tránh có
// 2 hàm sinh id na ná nhau nằm rải rác 2 nơi.
pub(crate) fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}")
}
