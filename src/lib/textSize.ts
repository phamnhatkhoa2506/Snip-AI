// Cỡ chữ toàn app: nhỏ / vừa (mặc định) / lớn — cho người mắt kém/màn hình
// độ phân giải cao thấy chữ hiện tại (nhiều chỗ dùng cỡ chữ cố định khá nhỏ,
// VD 10.5-13px) đọc thoải mái hơn, không cần zoom cả hệ điều hành.
//
// CÁCH LÀM: dùng CSS `zoom` trên toàn bộ <html> thay vì đổi từng class chữ —
// app dùng RẤT NHIỀU cỡ chữ cố định theo px (Tailwind arbitrary value, VD
// `text-[12.5px]`) rải khắp hàng trăm chỗ, không phải đơn vị `rem` co giãn
// theo font-size gốc — sửa lại toàn bộ thành rem là việc quá lớn so với lợi
// ích. `zoom` (WebView2/Chromium hỗ trợ tốt, dù không phải chuẩn CSS chính
// thức — không vấn đề gì vì app CHỈ chạy trên WebView2, không cần tương
// thích trình duyệt khác) phóng to/thu nhỏ TOÀN BỘ layout+chữ cùng lúc,
// giống hệt Ctrl+"+"/"-" trong Chrome, không cần đụng gì tới CSS có sẵn.
//
// Lưu localStorage — CHUNG cho mọi cửa sổ (main/result/history, cùng
// origin), nên đổi 1 lần ở Settings là mọi cửa sổ khác đồng bộ theo (cần tự
// apply lại ở mỗi cửa sổ lúc mount, xem +layout.svelte — localStorage không
// tự đồng bộ real-time giữa các cửa sổ đang mở sẵn, chỉ đọc đúng khi mở cửa
// sổ mới/reload, giống hệt cơ chế theme.ts).
//
// ⚠️ CỐ TÌNH KHÔNG áp dụng cho cửa sổ "overlay" (chọn vùng chụp) — overlay
// tính khung chọn trực tiếp từ `e.clientX`/`e.clientY` để suy ra toạ độ pixel
// THẬT trên màn hình (xem overlay/+page.svelte); `zoom` sẽ làm lệch phép
// tính đó, có thể chụp sai vùng. Xem điều kiện loại trừ theo route trong
// +layout.svelte.

export type TextSizeMode = "small" | "medium" | "large";

const STORAGE_KEY = "snip-ai:textSize";

const ZOOM_BY_MODE: Record<TextSizeMode, string> = {
  small: "0.9",
  medium: "1",
  large: "1.15",
};

export function loadTextSize(): TextSizeMode {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === "small" || raw === "medium" || raw === "large") return raw;
  } catch {
    // localStorage có thể bị chặn (private mode...) — rơi về mặc định, không throw
  }
  return "medium";
}

/** Áp dụng lên <html> của cửa sổ HIỆN TẠI — gọi lúc mount mọi route (trừ
 * overlay, xem giải thích ở đầu file). */
export function applyTextSize(mode: TextSizeMode): void {
  // `setProperty` thay vì gán thẳng `style.zoom` — `zoom` không phải thuộc
  // tính CSS chuẩn nên kiểu TypeScript của CSSStyleDeclaration không khai
  // báo sẵn, `setProperty` nhận chuỗi tên bất kỳ nên luôn hợp lệ về kiểu.
  document.documentElement.style.setProperty("zoom", ZOOM_BY_MODE[mode]);
}

export function setTextSize(mode: TextSizeMode): void {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // bỏ qua nếu không lưu được — vẫn apply cho phiên hiện tại
  }
  applyTextSize(mode);
}
