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
}

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

export const DEFAULT_SETTINGS: Settings = {
  geminiModel: DEFAULT_GEMINI_MODEL,
  reasoningEffort: "auto",
  autoCopyOnCapture: false,
  micAllowed: false,
  systemAudioAllowed: false,
  audioSnapSource: "mic",
  autoSpeakAudioAnswers: true,
  videoAudio: "none",
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
