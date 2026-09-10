// Cấu hình KHÔNG nhạy cảm (tên model...) lưu bằng localStorage — dùng chung
// giữa các cửa sổ vì cùng origin.
//
// ⚠️ Không có API key nào lưu ở đây. Từ khi chuyển sang đăng nhập Google
// (xem oauth.rs), người dùng phổ thông không còn tự nhập/quản lý API key
// nữa — mục "Nhà cung cấp AI (nâng cao)" (chọn provider, nhập key thủ công)
// đã bị GỠ HẲN khỏi UI vì quá phức tạp với đối tượng chính (học sinh/sinh
// viên, văn phòng). `provider` vẫn tồn tại trong `Settings` vì `ai.rs` dùng
// nó để định tuyến lệnh gọi AI, nhưng LUÔN là "gemini" trong thực tế sử dụng
// (không còn UI nào đổi được giá trị này nữa) — session đăng nhập Google chỉ
// hoạt động với route đó (xem ai.rs::ask_ai_gemini).

export type Provider = "nvidia" | "openai" | "anthropic" | "gemini";

export interface Settings {
  provider: Provider;
  nvidiaModel: string;
  /** Chỉ 1 số model NVIDIA hỗ trợ "reasoning" (VD DeepSeek-R1, QwQ, Kimi-K2).
   * Đa số model vision thường (VD Llama vision) KHÔNG có khái niệm này — nên
   * mặc định TẮT, người dùng tự bật khi biết chắc model của mình hỗ trợ. */
  nvidiaReasoningEnabled: boolean;
  nvidiaReasoningEffort: string;
  openaiModel: string;
  anthropicModel: string;
  geminiModel: string;
}

const STORAGE_KEY = "snip-ai:settings";

export const DEFAULT_SETTINGS: Settings = {
  // "gemini" — vì đây là provider đi qua backend khi đã đăng nhập Google
  // (xem ai.rs::ask_ai_gemini), đường DUY NHẤT cho người dùng phổ thông.
  provider: "gemini",
  nvidiaModel: "meta/llama-3.2-11b-vision-instruct",
  nvidiaReasoningEnabled: false,
  nvidiaReasoningEffort: "none",
  openaiModel: "gpt-4o",
  anthropicModel: "claude-sonnet-5",
  geminiModel: "gemini-3.6-flash",
};

/** Trường trong `Settings` chứa tên model của từng provider — chỉ dùng nội
 * bộ để `currentModel()` tra đúng field, không cần export ra ngoài nữa vì
 * không còn UI nào cho đổi provider. */
function modelFieldOf(provider: Provider): keyof Settings {
  switch (provider) {
    case "nvidia":
      return "nvidiaModel";
    case "openai":
      return "openaiModel";
    case "anthropic":
      return "anthropicModel";
    case "gemini":
      return "geminiModel";
  }
}

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
  const clean: Settings = {
    ...settings,
    nvidiaModel: settings.nvidiaModel.trim(),
    openaiModel: settings.openaiModel.trim(),
    anthropicModel: settings.anthropicModel.trim(),
    geminiModel: settings.geminiModel.trim(),
  };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(clean));
}

/** Tên model của provider đang chọn (đã trim — tránh lỗi từng gặp: model dán
 * dính khoảng trắng cuối khiến API trả HTTP 404 khó hiểu). */
export function currentModel(settings: Settings): string {
  return String(settings[modelFieldOf(settings.provider)] ?? "").trim();
}
