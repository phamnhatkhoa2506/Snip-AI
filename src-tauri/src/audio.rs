//! Lõi âm thanh dùng chung: thu MIC và/hoặc ÂM THANH HỆ THỐNG (WASAPI
//! loopback — thu đúng những gì đang phát ra loa, không cần driver ảo), trộn
//! lại thành 1 luồng PCM 16-bit đều nhịp, và PHÁT âm thanh ra loa. Dùng cho:
//! - Snap Audio (audio_snap.rs): 16kHz mono -> file WAV gửi Gemini.
//! - Video có tiếng (record.rs): 48kHz stereo -> track AAC trong MP4.
//! - Trò chuyện trực tiếp (live.rs): 16kHz mono gửi Live API + phát giọng AI.
//!
//! VÌ SAO có "đồng hồ trộn" riêng thay vì đẩy thẳng dữ liệu từ callback của
//! thiết bị: loopback của WASAPI KHÔNG gửi gói nào khi máy đang im lặng (không
//! có gì phát ra loa). Nếu đếm thời gian theo số mẫu nhận được (như encoder
//! video làm), mỗi quãng im lặng sẽ bị "nén mất" -> tiếng lệch khỏi hình. Đồng
//! hồ trộn chạy theo giờ thật: đến nhịp nào thì xuất đủ số mẫu của nhịp đó,
//! thiếu thì đệm 0 (im lặng), nên độ dài luồng ra luôn đúng bằng thời gian thật.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Frontend nhận ra lỗi "Windows chặn quyền micro" qua tiền tố này để hiện
/// nút mở thẳng trang cài đặt quyền riêng tư, thay vì chỉ in lỗi chung chung.
pub const MIC_PERMISSION_ERROR_PREFIX: &str = "MIC_PERMISSION_DENIED:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioSource {
    Mic,
    System,
}

impl AudioSource {
    /// Đọc danh sách nguồn frontend gửi lên ("mic"/"system") — bỏ qua giá trị
    /// lạ, bỏ trùng.
    pub fn parse_list(list: &[String]) -> Vec<AudioSource> {
        let mut out = Vec::new();
        for s in list {
            let src = match s.as_str() {
                "mic" => AudioSource::Mic,
                "system" => AudioSource::System,
                _ => continue,
            };
            if !out.contains(&src) {
                out.push(src);
            }
        }
        out
    }
}

/// Windows trả E_ACCESSDENIED khi tắt "Cho phép ứng dụng desktop dùng micro"
/// — cpal không map mã này sang `PermissionDenied` mà để `BackendError` kèm
/// thông điệp của hệ điều hành. Thông điệp đó được Windows DỊCH theo ngôn ngữ
/// máy ("Access is denied" / "Truy cập bị từ chối"...), nhưng hậu tố
/// "(os error 5)" thì cố định — dựa vào đó để nhận diện chắc chắn.
fn is_permission_error(e: &cpal::Error) -> bool {
    e.kind() == cpal::ErrorKind::PermissionDenied || e.to_string().contains("(os error 5)")
}

fn describe_error(source: AudioSource, e: &cpal::Error) -> String {
    match source {
        AudioSource::Mic if is_permission_error(e) => format!(
            "{MIC_PERMISSION_ERROR_PREFIX}Windows đang chặn ứng dụng dùng micro. Bật \"Cho phép ứng dụng desktop truy cập micro\" trong Cài đặt quyền riêng tư của Windows rồi thử lại."
        ),
        AudioSource::Mic => format!("Không mở được micro: {e}"),
        AudioSource::System => format!("Không thu được âm thanh hệ thống: {e}"),
    }
}

/// Đổi tần số lấy mẫu bằng nội suy tuyến tính, có lọc thông thấp 1 cực khi
/// HẠ tần số (VD 48kHz -> 16kHz) để bớt méo răng cưa. Đủ tốt cho giọng nói
/// gửi AI nghe/nhận dạng — không phải bộ resample chất lượng phòng thu.
struct Resampler {
    step: f64,
    pos: f64,
    last: f32,
    lp_alpha: Option<f32>,
    lp_state: f32,
}

impl Resampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        let lp_alpha = (in_rate > out_rate).then(|| {
            let cutoff = out_rate as f32 * 0.45;
            1.0 - (-2.0 * std::f32::consts::PI * cutoff / in_rate as f32).exp()
        });
        Self { step: in_rate as f64 / out_rate as f64, pos: 0.0, last: 0.0, lp_alpha, lp_state: 0.0 }
    }

    fn process(&mut self, input: &mut [f32], out: &mut VecDeque<f32>) {
        if let Some(alpha) = self.lp_alpha {
            for s in input.iter_mut() {
                self.lp_state += alpha * (*s - self.lp_state);
                *s = self.lp_state;
            }
        }
        // Dãy mở rộng: ext[0] = mẫu cuối của lần trước, ext[k] = input[k-1].
        // Nội suy giữa ext[i] và ext[i+1] cần i < n.
        let n = input.len();
        let mut pos = self.pos;
        while pos < n as f64 {
            let i = pos.floor() as usize;
            let frac = (pos - i as f64) as f32;
            let a = if i == 0 { self.last } else { input[i - 1] };
            let b = input[i];
            out.push_back(a + (b - a) * frac);
            pos += self.step;
        }
        self.pos = pos - n as f64;
        if let Some(&l) = input.last() {
            self.last = l;
        }
    }
}

/// Gộp các kênh về mono f32 [-1, 1]. Trả `false` với định dạng mẫu lạ (hiếm
/// gặp — WASAPI chế độ shared gần như luôn là f32).
fn to_mono_f32(data: &cpal::Data, channels: usize, out: &mut Vec<f32>) -> bool {
    fn fold<T: Copy>(samples: &[T], channels: usize, out: &mut Vec<f32>, conv: impl Fn(T) -> f32) {
        let ch = channels.max(1);
        for frame in samples.chunks(ch) {
            let sum: f32 = frame.iter().map(|&s| conv(s)).sum();
            out.push(sum / frame.len() as f32);
        }
    }
    use cpal::SampleFormat as F;
    match data.sample_format() {
        F::F32 => fold(data.as_slice::<f32>().unwrap_or(&[]), channels, out, |s| s),
        F::F64 => fold(data.as_slice::<f64>().unwrap_or(&[]), channels, out, |s| s as f32),
        F::I16 => fold(data.as_slice::<i16>().unwrap_or(&[]), channels, out, |s| s as f32 / 32768.0),
        F::I32 => fold(data.as_slice::<i32>().unwrap_or(&[]), channels, out, |s| s as f32 / 2_147_483_648.0),
        F::U8 => fold(data.as_slice::<u8>().unwrap_or(&[]), channels, out, |s| (s as f32 - 128.0) / 128.0),
        F::U16 => fold(data.as_slice::<u16>().unwrap_or(&[]), channels, out, |s| (s as f32 - 32768.0) / 32768.0),
        _ => return false,
    }
    true
}

type SharedQueue = Arc<Mutex<VecDeque<f32>>>;

fn open_source_stream(source: AudioSource, out_rate: u32, queue: SharedQueue) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    // Âm thanh hệ thống = mở THIẾT BỊ PHÁT dưới dạng input — cpal tự bật chế
    // độ loopback của WASAPI trong trường hợp này.
    let device = match source {
        AudioSource::Mic => host.default_input_device().ok_or("Không tìm thấy micro nào trên máy.")?,
        AudioSource::System => host
            .default_output_device()
            .ok_or("Không tìm thấy loa/thiết bị phát âm thanh nào trên máy.")?,
    };
    let supported = match source {
        AudioSource::Mic => device.default_input_config(),
        AudioSource::System => device.default_output_config(),
    }
    .map_err(|e| describe_error(source, &e))?;

    let sample_format = supported.sample_format();
    let config = supported.config();
    let channels = config.channels as usize;
    let mut resampler = Resampler::new(config.sample_rate, out_rate);
    let mut mono: Vec<f32> = Vec::new();

    let stream = device
        .build_input_stream_raw(
            config,
            sample_format,
            move |data: &cpal::Data, _info: &cpal::InputCallbackInfo| {
                mono.clear();
                if !to_mono_f32(data, channels, &mut mono) {
                    return;
                }
                let mut q = queue.lock().unwrap();
                resampler.process(&mut mono, &mut q);
            },
            move |err| eprintln!("[snip-ai][audio] Lỗi luồng thu ({source:?}): {err}"),
            Some(Duration::from_secs(3)),
        )
        .map_err(|e| describe_error(source, &e))?;
    stream.play().map_err(|e| describe_error(source, &e))?;
    Ok(stream)
}

pub struct CaptureOptions {
    pub sources: Vec<AudioSource>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Mỗi lần gọi `sink` nhận đúng `frame_ms` mili-giây âm thanh.
    pub frame_ms: u32,
    /// `true`: nguồn nào mở lỗi là báo lỗi luôn (Snap Audio, trò chuyện trực
    /// tiếp — người dùng đã chủ động chọn nguồn đó). `false`: bỏ qua nguồn
    /// lỗi, vẫn chạy với phần còn lại, kể cả khi không còn nguồn nào (xuất
    /// toàn im lặng) — dùng cho video, nơi encoder đã bật track tiếng thì
    /// PHẢI luôn nhận đủ dữ liệu, nếu không chốt file sẽ treo.
    pub strict: bool,
}

/// Tay cầm phiên thu — dừng khi gọi `stop()` hoặc khi bị drop.
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Bắt đầu thu. `sink` chạy trên luồng riêng của phiên thu, nhận PCM 16-bit
/// xen kẽ kênh, đúng `frame_ms` mỗi lần, đều nhịp theo giờ thật. Trả về cảnh
/// báo cho các nguồn bị bỏ qua (chỉ khi `strict = false`).
pub fn start_capture<F>(opts: CaptureOptions, mut sink: F) -> Result<(CaptureHandle, Vec<String>), String>
where
    F: FnMut(&[i16]) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<Vec<String>, String>>();

    let thread = std::thread::spawn(move || {
        let mut streams = Vec::new();
        let mut queues: Vec<SharedQueue> = Vec::new();
        let mut warnings = Vec::new();
        for &source in &opts.sources {
            let queue: SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
            match open_source_stream(source, opts.sample_rate, queue.clone()) {
                Ok(stream) => {
                    streams.push(stream);
                    queues.push(queue);
                }
                Err(e) if opts.strict => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
                Err(e) => warnings.push(e),
            }
        }
        let _ = ready_tx.send(Ok(warnings));

        let frame_len = (opts.sample_rate as u64 * opts.frame_ms as u64 / 1000).max(1) as usize;
        let frame_dur = Duration::from_millis(opts.frame_ms as u64);
        let channels = opts.channels.max(1) as usize;
        let mut mix = vec![0f32; frame_len];
        let mut out: Vec<i16> = Vec::with_capacity(frame_len * channels);

        // Đệm trước ~1 nhịp + 40ms: thiết bị giao dữ liệu theo từng cục ~10ms
        // lệch pha với nhịp trộn, không chừa đệm thì thỉnh thoảng hụt vài ms
        // giữa câu nói -> nghe "lụp bụp".
        std::thread::sleep(frame_dur + Duration::from_millis(40));
        let started = Instant::now();
        let mut n: u32 = 0;
        while !stop_thread.load(Ordering::SeqCst) {
            n += 1;
            let target = started + frame_dur * n;
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            }

            mix.fill(0.0);
            for queue in &queues {
                let mut q = queue.lock().unwrap();
                for slot in mix.iter_mut() {
                    match q.pop_front() {
                        Some(v) => *slot += v,
                        None => break,
                    }
                }
                // Đồng hồ thiết bị chạy nhanh hơn đồng hồ hệ thống 1 chút là
                // chuyện thường — không cắt bớt thì độ trễ cứ thế tăng dần.
                let excess = q.len().saturating_sub(frame_len * 4);
                q.drain(..excess);
            }

            out.clear();
            for &s in &mix {
                let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                for _ in 0..channels {
                    out.push(v);
                }
            }
            sink(&out);
        }
        drop(streams);
    });

    let warnings = ready_rx.recv().map_err(|_| "Luồng thu âm dừng bất ngờ.".to_string())??;
    Ok((CaptureHandle { stop, thread: Some(thread) }, warnings))
}

/// Thử mở micro ~150ms để kiểm tra quyền truy cập — dùng khi người dùng bật
/// quyền micro trong Cài đặt, báo lỗi ngay thay vì đợi tới lúc thu thật.
pub fn probe_microphone() -> Result<(), String> {
    let (handle, _) = start_capture(
        CaptureOptions { sources: vec![AudioSource::Mic], sample_rate: 16_000, channels: 1, frame_ms: 50, strict: true },
        |_| {},
    )?;
    std::thread::sleep(Duration::from_millis(150));
    handle.stop();
    Ok(())
}

/// Mức âm lượng 0..1 (RMS, đã nâng lên để thanh hiển thị nhạy với giọng nói
/// bình thường) — chỉ để vẽ thanh sóng trên giao diện.
pub fn level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    let rms = (sum / samples.len() as f64).sqrt() as f32;
    (rms * 4.0).min(1.0)
}

pub fn wav_from_pcm16(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * channels as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn pcm16_from_le_bytes(bytes: &[u8]) -> Vec<i16> {
    bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// Phát PCM ra loa mặc định — dùng cho giọng AI ở chế độ trò chuyện trực
/// tiếp (Live API gửi về từng mẩu PCM 24kHz, nhanh hơn thời gian thực, nên
/// cần hàng đợi phát riêng thay vì phát từng mẩu rời rạc).
pub struct Playback {
    queue: SharedQueue,
    device_rate: u32,
    resampler: Mutex<Option<(u32, Resampler)>>,
    /// Thời điểm hàng đợi vừa phát hết — dùng để tính "đuôi" tiếng vọng lại
    /// trong phòng sau khi AI ngừng nói (xem `is_active`).
    drained_at: Arc<Mutex<Option<Instant>>>,
    _stream: cpal::Stream,
}

impl Playback {
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("Không tìm thấy loa/tai nghe nào để phát giọng AI.")?;
        let supported = device.default_output_config().map_err(|e| format!("Không mở được loa: {e}"))?;
        let config = supported.config();
        let device_rate = config.sample_rate;
        let channels = config.channels as usize;

        let queue: SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
        let drained_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let q = queue.clone();
        let drained = drained_at.clone();

        // Luồng PHÁT được cpal mở với cờ tự chuyển định dạng (AUTOCONVERTPCM)
        // nên luôn dùng được f32, không cần xử lý từng định dạng như luồng thu.
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    let mut q = q.lock().unwrap();
                    let had_audio = !q.is_empty();
                    for frame in data.chunks_mut(channels.max(1)) {
                        let v = q.pop_front().unwrap_or(0.0);
                        frame.fill(v);
                    }
                    if had_audio && q.is_empty() {
                        *drained.lock().unwrap() = Some(Instant::now());
                    }
                },
                |err| eprintln!("[snip-ai][audio] Lỗi luồng phát: {err}"),
                Some(Duration::from_secs(3)),
            )
            .map_err(|e| format!("Không mở được loa: {e}"))?;
        stream.play().map_err(|e| format!("Không phát được âm thanh: {e}"))?;

        Ok(Self { queue, device_rate, resampler: Mutex::new(None), drained_at, _stream: stream })
    }

    pub fn push_pcm16(&self, samples: &[i16], rate: u32) {
        let mut guard = self.resampler.lock().unwrap();
        if guard.as_ref().map(|(r, _)| *r) != Some(rate) {
            *guard = Some((rate, Resampler::new(rate, self.device_rate)));
        }
        let (_, resampler) = guard.as_mut().unwrap();
        let mut input: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        let mut q = self.queue.lock().unwrap();
        resampler.process(&mut input, &mut q);
    }

    /// Bỏ ngay phần giọng AI chưa phát — khi người dùng nói chen ngang.
    pub fn clear(&self) {
        self.queue.lock().unwrap().clear();
    }

    /// Đang phát, hoặc vừa phát xong chưa quá `tail` (tiếng vọng còn đọng
    /// trong phòng vẫn có thể lọt vào micro).
    pub fn is_active(&self, tail: Duration) -> bool {
        if !self.queue.lock().unwrap().is_empty() {
            return true;
        }
        self.drained_at.lock().unwrap().is_some_and(|t| t.elapsed() < tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5).collect()
    }

    #[test]
    fn resample_down_keeps_length_and_amplitude() {
        let mut input = sine(48_000, 440.0, 1.0);
        let mut out = VecDeque::new();
        let mut r = Resampler::new(48_000, 16_000);
        // Chia nhiều cục nhỏ như callback thật, kiểm tra ráp nối giữa các cục.
        for chunk in input.chunks_mut(480) {
            r.process(chunk, &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len = {}", out.len());
        let peak = out.iter().skip(1000).fold(0f32, |m, &v| m.max(v.abs()));
        assert!(peak > 0.45 && peak < 0.55, "peak = {peak}");
    }

    #[test]
    fn resample_up_keeps_length() {
        let mut input = sine(24_000, 300.0, 0.5);
        let mut out = VecDeque::new();
        let mut r = Resampler::new(24_000, 48_000);
        for chunk in input.chunks_mut(240) {
            r.process(chunk, &mut out);
        }
        assert!((out.len() as i64 - 24_000).abs() <= 2, "len = {}", out.len());
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = wav_from_pcm16(&[0, 1000, -1000, 32767], 16_000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize, wav.len() - 8);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(pcm16_from_le_bytes(&wav[44..]), vec![0, 1000, -1000, 32767]);
    }

    /// Không có nguồn nào (video khi mọi nguồn lỗi) vẫn phải xuất im lặng
    /// đều nhịp, đúng độ dài theo giờ thật.
    #[test]
    fn mixer_without_sources_emits_realtime_silence() {
        let got = Arc::new(Mutex::new(Vec::<i16>::new()));
        let sink = got.clone();
        let (handle, warnings) = start_capture(
            CaptureOptions { sources: vec![], sample_rate: 16_000, channels: 2, frame_ms: 20, strict: false },
            move |f| sink.lock().unwrap().extend_from_slice(f),
        )
        .unwrap();
        assert!(warnings.is_empty());
        std::thread::sleep(Duration::from_millis(560));
        handle.stop();
        let got = got.lock().unwrap();
        assert_eq!(got.len() % (320 * 2), 0, "mỗi nhịp 20ms stereo = 640 mẫu");
        let frames = got.len() / 640;
        // 560ms - 60ms đệm trước ≈ 25 nhịp; chừa sai số lịch hệ điều hành.
        assert!((20..=27).contains(&frames), "frames = {frames}");
        assert!(got.iter().all(|&s| s == 0));
    }

    /// Mở THẬT loa mặc định, phát 0.4s âm nhỏ 24kHz (đúng định dạng Live API
    /// trả về) — kiểm tra hàng đợi phát, trạng thái "đang phát" và `clear()`.
    #[test]
    #[ignore]
    fn real_playback_drains_and_clears() {
        let playback = Playback::start().expect("không mở được loa");
        let tone: Vec<i16> = (0..9_600).map(|i| ((i as f32 * 0.05).sin() * 1500.0) as i16).collect();
        playback.push_pcm16(&tone, 24_000);
        assert!(playback.is_active(Duration::ZERO), "vừa đẩy tiếng vào mà không thấy đang phát");
        std::thread::sleep(Duration::from_millis(900));
        assert!(!playback.is_active(Duration::ZERO), "phát 0.4s mà sau 0.9s vẫn chưa hết");
        assert!(playback.is_active(Duration::from_secs(5)), "đuôi tiếng vọng phải tính từ lúc vừa phát xong");
        playback.push_pcm16(&tone, 24_000);
        playback.clear();
        assert!(!playback.is_active(Duration::ZERO), "clear() phải bỏ ngay phần chưa phát");
    }

    /// Mở THẬT micro + loopback trên máy đang chạy test — `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_devices_capture() {
        for sources in [vec![AudioSource::System], vec![AudioSource::Mic], vec![AudioSource::Mic, AudioSource::System]] {
            let got = Arc::new(Mutex::new(Vec::<i16>::new()));
            let sink = got.clone();
            let result = start_capture(
                CaptureOptions { sources: sources.clone(), sample_rate: 16_000, channels: 1, frame_ms: 100, strict: true },
                move |f| sink.lock().unwrap().extend_from_slice(f),
            );
            match result {
                Ok((handle, _)) => {
                    std::thread::sleep(Duration::from_millis(1100));
                    handle.stop();
                    let got = got.lock().unwrap();
                    let lvl = level(&got);
                    println!("{sources:?}: {} mẫu (~{:.2}s), level {lvl:.3}", got.len(), got.len() as f32 / 16_000.0);
                    assert!(got.len() >= 14_000, "quá ít mẫu: {}", got.len());
                }
                Err(e) => println!("{sources:?}: LỖI {e}"),
            }
        }
    }
}
