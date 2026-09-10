// Chế độ giao diện: sáng / tối / hệ thống (mặc định). "Hệ thống" nghĩa là để
// CSS tự quyết theo `prefers-color-scheme` (xem app.css) — không set thuộc
// tính gì lên <html>. Chọn "sáng"/"tối" thủ công thì ép cứng bằng thuộc tính
// `data-theme`, đè lên `prefers-color-scheme` bất kể Windows đang set gì.
//
// Lưu localStorage — CHUNG cho mọi cửa sổ (main/overlay/result, cùng origin),
// nên đổi 1 lần ở Settings là mọi cửa sổ khác đồng bộ theo (cần tự apply lại
// ở mỗi cửa sổ lúc mount, xem +layout.svelte — localStorage không tự đồng bộ
// real-time giữa các cửa sổ đang mở sẵn, chỉ đọc đúng khi mở cửa sổ mới/reload).

export type ThemeMode = "light" | "dark" | "system";

const STORAGE_KEY = "snip-ai:theme";

export function loadTheme(): ThemeMode {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    // localStorage có thể bị chặn (private mode...) — rơi về mặc định, không throw
  }
  return "system";
}

/** Áp dụng lên <html> của cửa sổ HIỆN TẠI — gọi lúc mount mọi route. */
export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  if (mode === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", mode);
  }
}

export function setTheme(mode: ThemeMode): void {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // bỏ qua nếu không lưu được — vẫn apply cho phiên hiện tại
  }
  applyTheme(mode);
}
