// Cấu hình KHÔNG nhạy cảm (tên model...) lưu bằng localStorage — dùng chung
// giữa các cửa sổ vì cùng origin.
//
// ⚠️ Không có API key nào lưu ở đây. Bản v0 đầu tiên từng hỗ trợ nhiều "nhà
// cung cấp AI" (NVIDIA NIM/OpenAI/Anthropic/Gemini, tự nhập key riêng từng
// nơi) — đã BỎ HẲN (không chỉ ẩn UI) khi chuyển sang đăng nhập Google (xem
// oauth.rs): key thật quản lý ở backend, người dùng phổ thông không cần
// biết/chọn provider gì cả. Chỉ còn Gemini.

/** Mức độ "suy luận ẩn" (thinking) Gemini 3 làm trước khi trả lời — tham số
 * `thinkingLevel` của Google, 4 mức chính thức MINIMAL/LOW/MEDIUM/HIGH (một
 * số model KHÔNG hỗ trợ MINIMAL, xem retry-khi-400 trong ai.rs) + "auto"
 * (KHÔNG ép field này, để model tự chọn mặc định — hành vi trước đây sau khi
 * bỏ hẳn việc ép cứng "minimal" cho mọi câu hỏi, xem lịch sử ai.rs). Cho
 * người dùng TỰ CHỌN thay vì áp 1 mức chung: suy luận nhiều hơn = chính xác
 * hơn cho bài khó nhưng chậm hơn, câu hỏi đơn giản (OCR, dịch...) có thể chủ
 * động chọn mức thấp để trả lời nhanh hơn. */
export type ReasoningEffort = "auto" | "minimal" | "low" | "medium" | "high";

export interface Settings {
  geminiModel: string;
  reasoningEffort: ReasoningEffort;
  /** Tự động chép ảnh/video vào clipboard hệ thống ngay sau khi chụp/quay
   * xong — TẮT mặc định (nhiều người chỉ cần ảnh/video nằm trong app để hỏi
   * AI, không phải lúc nào cũng muốn nó "chiếm" luôn clipboard, có thể đè mất
   * nội dung khác vừa copy trước đó). Đọc ở overlay/+page.svelte ngay lúc
   * chụp/quay (xem clipboard_copy.rs phía Rust — nơi thực sự ghi clipboard). */
  autoCopyOnCapture: boolean;
  /** Người dùng đã CHO PHÉP app dùng micro (Snap Audio, trò chuyện trực
   * tiếp...). Windows không tự hỏi quyền cho app desktop, nên app tự xin —
   * TẮT mặc định, bật lên thì kiểm tra thật bằng `probe_microphone`. */
  micAllowed: boolean;
  /** Cho phép thu ÂM THANH HỆ THỐNG (mọi thứ đang phát ra loa). Windows
   * không chặn loại này, nhưng vẫn cần người dùng tự bật vì nó nghe được
   * mọi âm thanh trên máy. */
  systemAudioAllowed: boolean;
  /** Nguồn mặc định của Snap Audio. */
  audioSnapSource: AudioSnapSource;
  /** Hỏi về 1 đoạn ghi âm (Snap Audio) thì TỰ ĐỌC câu trả lời bằng giọng
   * nói — "hỏi bằng giọng, nghe trả lời bằng giọng". Mọi câu trả lời khác
   * vẫn có nút loa để bấm đọc thủ công. */
  autoSpeakAudioAnswers: boolean;
  /** Ghi kèm tiếng khi QUAY VIDEO — TẮT mặc định (video cũ vốn không có
   * tiếng, bật lên file nặng hơn chút và cần quyền micro/âm thanh máy). */
  videoAudio: VideoAudioChoice;
  /** Trò chuyện trực tiếp: đang đeo TAI NGHE -> gửi thẳng tiếng micro kể cả
   * lúc AI đang nói (nói chen ngang được). Dùng loa ngoài (mặc định) thì
   * lúc AI nói app gửi im lặng thay cho micro, tránh AI nghe lại chính giọng
   * mình qua loa rồi tự ngắt lời (xem live.rs). */
  liveHeadphones: boolean;
  /** Model ĐỌC câu trả lời thành giọng nói (nút loa / tự đọc) — xem
   * TTS_MODEL_OPTIONS. */
  ttsModel: string;
  /** Model TRÒ CHUYỆN TRỰC TIẾP bằng giọng nói — xem LIVE_MODEL_OPTIONS. */
  liveModel: string;
  /** Giọng AI — dùng CHUNG cho cả đọc câu trả lời lẫn trò chuyện trực tiếp
   * (cùng bộ giọng dựng sẵn của Gemini), để AI luôn "cùng 1 giọng". */
  voiceName: string;
  /** Phụ đề trực tiếp (cửa sổ caption): chép lời hay dịch, nguồn âm thanh,
   * ngôn ngữ đích của chế độ dịch, cỡ chữ (px). */
  captionMode: CaptionMode;
  captionSource: AudioSnapSource;
  captionTargetLang: string;
  captionFontSize: number;
}

export type CaptionMode = "transcribe" | "translate";

/** Ngôn ngữ đích của chế độ dịch — TRÙNG TARGET_LANGS trong caption.rs. */
export const CAPTION_LANG_OPTIONS: { value: string; title: string }[] = [
  { value: "vi", title: "Tiếng Việt" },
  { value: "en", title: "Tiếng Anh" },
  { value: "ja", title: "Tiếng Nhật" },
  { value: "ko", title: "Tiếng Hàn" },
  { value: "zh", title: "Tiếng Trung" },
  { value: "fr", title: "Tiếng Pháp" },
  { value: "de", title: "Tiếng Đức" },
  { value: "es", title: "Tiếng Tây Ban Nha" },
  { value: "th", title: "Tiếng Thái" },
  { value: "id", title: "Tiếng Indonesia" },
  { value: "ru", title: "Tiếng Nga" },
  { value: "pt", title: "Tiếng Bồ Đào Nha" },
  { value: "it", title: "Tiếng Ý" },
  { value: "hi", title: "Tiếng Hindi" },
  { value: "ar", title: "Tiếng Ả Rập" },
];

export type AudioSnapSource = "mic" | "system" | "both";
export type VideoAudioChoice = "none" | AudioSnapSource;

const STORAGE_KEY = "snip-ai:settings";

/** Model mặc định — trùng GEMINI_MODEL của backend (wrangler.toml). */
export const DEFAULT_GEMINI_MODEL = "gemini-3.6-flash";

/** Các model người dùng chọn được trong Cài đặt -> "Mô hình AI". CHỈ gồm
 * model chat ỔN ĐỊNH còn FREE TIER của Gemini API (đã đối chiếu trang
 * models/pricing của Google, 09/2026): bỏ dòng Pro (không còn free tier),
 * dòng 2.5 (Google chỉ còn cho người dùng cũ) và bản preview (đã có bản ổn
 * định mới hơn). Backend có danh sách cho phép TƯƠNG ỨNG
 * (DEFAULT_ALLOWED_CHAT_MODELS trong backend/src/index.ts) — thêm/bớt model
 * phải sửa CẢ HAI nơi, model ngoài danh sách backend sẽ lặng lẽ bị thay
 * bằng model mặc định. */
export const GEMINI_MODEL_OPTIONS: { value: string; title: string; description: string }[] = [
  { value: "gemini-3.8-flash", title: "Gemini 3.8 Flash", description: "Mới nhất, thông minh nhất — bài khó, lập trình, suy luận nhiều bước" },
  { value: "gemini-3.7-flash", title: "Gemini 3.7 Flash", description: "Thế hệ trước, vẫn mạnh cho lập trình và suy luận" },
  { value: "gemini-3.6-flash", title: "Gemini 3.6 Flash", description: "Cân bằng tốc độ và độ chính xác (mặc định)" },
  { value: "gemini-3.5-flash", title: "Gemini 3.5 Flash", description: "Nhanh, ổn định cho câu hỏi thường ngày" },
  { value: "gemini-3.5-flash-lite", title: "Gemini 3.5 Flash-Lite", description: "Nhanh nhất — OCR, dịch, hỏi nhanh" },
  { value: "gemini-3.1-flash-lite", title: "Gemini 3.1 Flash-Lite", description: "Nhẹ, tiết kiệm — dự phòng khi model khác quá tải" },
];

interface Option {
  value: string;
  title: string;
  description: string;
}

/** Model đọc giọng nói (TTS) — chỉ bản ỔN ĐỊNH còn free tier (09/2026), bỏ
 * bản preview/cũ. Backend có danh sách cho phép TƯƠNG ỨNG
 * (ALLOWED_TTS_MODELS trong backend/src/index.ts) — sửa CẢ HAI nơi. */
export const DEFAULT_TTS_MODEL = "gemini-3.8-flash-lite-tts";
export const TTS_MODEL_OPTIONS: Option[] = [
  { value: "gemini-3.8-flash-lite-tts", title: "Gemini 3.8 Flash-Lite TTS", description: "Nhanh, nhẹ quota (mặc định)" },
  { value: "gemini-3.8-flash-tts", title: "Gemini 3.8 Flash TTS", description: "Giọng tự nhiên, diễn cảm hơn — chậm hơn chút" },
  { value: "gemini-3.1-flash-tts-preview", title: "Gemini 3.1 Flash TTS (preview)", description: "Bản cũ hơn — dự phòng khi 3.8 quá tải" },
  { value: "gemini-2.5-flash-preview-tts", title: "Gemini 2.5 Flash TTS (preview)", description: "Bản cũ nhất — dự phòng" },
];

/** Model trò chuyện trực tiếp (Live API) — cùng quy tắc chọn như trên,
 * backend: ALLOWED_LIVE_MODELS. */
export const DEFAULT_LIVE_MODEL = "gemini-3.8-live";
export const LIVE_MODEL_OPTIONS: Option[] = [
  { value: "gemini-3.8-live", title: "Gemini 3.8 Live", description: "Phản hồi nhanh, tự nhiên như gọi điện (mặc định)" },
  { value: "gemini-3.8-live-extended-thinking", title: "Gemini 3.8 Live — suy luận sâu", description: "Nghĩ kỹ hơn trước khi nói — hợp câu hỏi khó, trả lời chậm hơn" },
  { value: "gemini-3.1-flash-live-preview", title: "Gemini 3.1 Flash Live (preview)", description: "Bản cũ hơn, dự phòng khi 3.8 quá tải — Google khuyên dùng 3.8" },
];

/** 30 giọng dựng sẵn của Gemini (dùng được cho cả TTS lẫn Live, đều nói
 * được tiếng Việt). `description` dịch từ mô tả phong cách chính thức của
 * Google (VD Kore — "Firm"). Tên giọng là định danh gửi thẳng cho API —
 * KHÔNG dịch/đổi. */
export const DEFAULT_VOICE = "Kore";
export const VOICE_OPTIONS: Option[] = [
  { value: "Kore", title: "Kore", description: "Chắc chắn, rõ ràng (mặc định)" },
  { value: "Zephyr", title: "Zephyr", description: "Tươi sáng" },
  { value: "Puck", title: "Puck", description: "Vui tươi" },
  { value: "Charon", title: "Charon", description: "Rõ ràng, kiểu thuyết minh" },
  { value: "Fenrir", title: "Fenrir", description: "Hào hứng" },
  { value: "Leda", title: "Leda", description: "Trẻ trung" },
  { value: "Orus", title: "Orus", description: "Chắc chắn" },
  { value: "Aoede", title: "Aoede", description: "Nhẹ nhàng, thoải mái" },
  { value: "Callirrhoe", title: "Callirrhoe", description: "Thư thái" },
  { value: "Autonoe", title: "Autonoe", description: "Tươi sáng" },
  { value: "Enceladus", title: "Enceladus", description: "Hơi thở nhẹ" },
  { value: "Iapetus", title: "Iapetus", description: "Trong trẻo" },
  { value: "Umbriel", title: "Umbriel", description: "Thư thái" },
  { value: "Algieba", title: "Algieba", description: "Mượt mà" },
  { value: "Despina", title: "Despina", description: "Mượt mà" },
  { value: "Erinome", title: "Erinome", description: "Trong trẻo" },
  { value: "Algenib", title: "Algenib", description: "Trầm khàn" },
  { value: "Rasalgethi", title: "Rasalgethi", description: "Kiểu thuyết minh" },
  { value: "Laomedeia", title: "Laomedeia", description: "Vui tươi" },
  { value: "Achernar", title: "Achernar", description: "Nhẹ nhàng" },
  { value: "Alnilam", title: "Alnilam", description: "Chắc chắn" },
  { value: "Schedar", title: "Schedar", description: "Đều đặn" },
  { value: "Gacrux", title: "Gacrux", description: "Chín chắn" },
  { value: "Pulcherrima", title: "Pulcherrima", description: "Thẳng thắn" },
  { value: "Achird", title: "Achird", description: "Thân thiện" },
  { value: "Zubenelgenubi", title: "Zubenelgenubi", description: "Tự nhiên, xuề xoà" },
  { value: "Vindemiatrix", title: "Vindemiatrix", description: "Dịu dàng" },
  { value: "Sadachbia", title: "Sadachbia", description: "Sôi nổi" },
  { value: "Sadaltager", title: "Sadaltager", description: "Hiểu biết" },
  { value: "Sulafat", title: "Sulafat", description: "Ấm áp" },
];

export const DEFAULT_SETTINGS: Settings = {
  geminiModel: DEFAULT_GEMINI_MODEL,
  reasoningEffort: "auto",
  autoCopyOnCapture: false,
  micAllowed: false,
  systemAudioAllowed: false,
  audioSnapSource: "mic",
  autoSpeakAudioAnswers: true,
  videoAudio: "none",
  liveHeadphones: false,
  ttsModel: DEFAULT_TTS_MODEL,
  liveModel: DEFAULT_LIVE_MODEL,
  voiceName: DEFAULT_VOICE,
  captionMode: "transcribe",
  captionSource: "system",
  captionTargetLang: "vi",
  captionFontSize: 18,
};

/** Danh sách nguồn thật sẽ thu cho 1 lựa chọn — CHỈ gồm những nguồn đã được
 * cho phép. Trả về mảng rỗng nếu lựa chọn cần quyền chưa được bật. */
export function allowedAudioSources(s: Settings, choice: AudioSnapSource): ("mic" | "system")[] {
  const wanted: ("mic" | "system")[] = choice === "both" ? ["mic", "system"] : [choice];
  return wanted.filter((src) => (src === "mic" ? s.micAllowed : s.systemAudioAllowed));
}

/** Rust báo lỗi thiếu quyền micro bằng tiền tố này (xem audio.rs). */
export const MIC_PERMISSION_ERROR_PREFIX = "MIC_PERMISSION_DENIED:";

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(settings: Settings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...settings, geminiModel: settings.geminiModel.trim() }));
}

/** Tên model đang dùng — luôn là 1 model TRONG danh sách GEMINI_MODEL_OPTIONS.
 * Giá trị đã lưu không còn hợp lệ (bản cũ từng cho gõ tên model tay, hoặc
 * model đã bị gỡ khỏi danh sách) thì rơi về mặc định thay vì gọi 1 model
 * không tồn tại rồi báo lỗi 404 khó hiểu. */
export function currentModel(settings: Settings): string {
  const model = (settings.geminiModel ?? "").trim();
  return GEMINI_MODEL_OPTIONS.some((o) => o.value === model) ? model : DEFAULT_GEMINI_MODEL;
}

function pick(options: Option[], value: string | undefined, fallback: string): string {
  const v = (value ?? "").trim();
  return options.some((o) => o.value === v) ? v : fallback;
}

/** Luôn trả về giá trị NẰM TRONG danh sách — cùng lý do với `currentModel`. */
export function currentTtsModel(s: Settings): string {
  return pick(TTS_MODEL_OPTIONS, s.ttsModel, DEFAULT_TTS_MODEL);
}

export function currentLiveModel(s: Settings): string {
  return pick(LIVE_MODEL_OPTIONS, s.liveModel, DEFAULT_LIVE_MODEL);
}

export function currentVoice(s: Settings): string {
  return pick(VOICE_OPTIONS, s.voiceName, DEFAULT_VOICE);
}

/** Danh sách 5 lựa chọn hiện trong UI Cài đặt (result/+page.svelte và
 * +page.svelte) — 1 nguồn DUY NHẤT, tránh lặp lại nhãn/mô tả ở nhiều nơi rồi
 * lệch nhau khi sửa sau này. */
export const REASONING_EFFORT_OPTIONS: { value: ReasoningEffort; title: string; description: string }[] = [
  { value: "auto", title: "Tự động", description: "Để Gemini tự chọn mức phù hợp (mặc định)" },
  { value: "minimal", title: "Tối thiểu", description: "Nhanh nhất — chỉ hợp câu hỏi rất đơn giản" },
  { value: "low", title: "Thấp", description: "Nhanh — OCR, dịch, hỏi nhanh" },
  { value: "medium", title: "Vừa", description: "Cân bằng tốc độ/độ chính xác" },
  { value: "high", title: "Cao", description: "Suy luận kỹ nhất — bài toán/chứng minh nhiều bước" },
];
