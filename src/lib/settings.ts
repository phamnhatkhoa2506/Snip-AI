// Cấu hình KHÔNG nhạy cảm (tên model...) lưu bằng localStorage — dùng chung
// giữa các cửa sổ vì cùng origin.
//
// ⚠️ Không có API key nào lưu ở đây. Bản v0 đầu tiên từng hỗ trợ nhiều "nhà
// cung cấp AI" (NVIDIA NIM/OpenAI/Anthropic/Gemini, tự nhập key riêng từng
// nơi) — đã BỎ HẲN (không chỉ ẩn UI) khi chuyển sang đăng nhập Google (xem
// oauth.rs): key thật quản lý ở backend, người dùng phổ thông không cần
// biết/chọn provider gì cả. Chỉ còn Gemini.

export interface Settings {
  geminiModel: string;
}

const STORAGE_KEY = "snip-ai:settings";

export const DEFAULT_SETTINGS: Settings = {
  geminiModel: "gemini-3.6-flash",
};

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

/** Tên model đang dùng (đã trim — tránh lỗi từng gặp: model dán dính khoảng
 * trắng cuối khiến API trả HTTP 404 khó hiểu). */
export function currentModel(settings: Settings): string {
  return settings.geminiModel.trim();
}
