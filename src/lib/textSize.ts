// Cỡ chữ TRONG BONG BÓNG CHAT (câu hỏi + câu trả lời AI): nhỏ / vừa (mặc
// định) / lớn — cho người mắt kém/màn hình độ phân giải cao thấy chữ hiện
// tại (12.5px cố định) đọc thoải mái hơn.
//
// ⚠️ THỬ dùng CSS `zoom` trên toàn bộ <html> TRƯỚC — phóng gọn cả layout lẫn
// chữ cùng lúc — nhưng gặp lỗi thực tế NGHIÊM TRỌNG: cửa sổ có KÍCH THƯỚC
// VẬT LÝ CỐ ĐỊNH (Rust set lúc mở, xem commands.rs), `zoom` chỉ phóng to nội
// dung HIỂN THỊ chứ không tự phóng theo cửa sổ — phóng chữ lên thì các phần
// cố định (header, hàng nút bấm, ô nhập ở đáy) bị đẩy tràn ra ngoài/che mất
// lẫn nhau, vỡ hẳn giao diện. Đổi hướng: CHỈ phóng to chữ BÊN TRONG bong
// bóng chat (câu hỏi/câu trả lời) — nơi đã nằm trong vùng CUỘN ĐƯỢC
// (ScrollArea, xem result/+page.svelte), phóng to chữ ở đó chỉ làm mỗi bong
// bóng cao lên, cuộn nhiều hơn — KHÔNG đụng gì tới bố cục cố định còn lại
// (header/nút bấm/ô nhập luôn giữ đúng kích thước ban đầu).
//
// CÁCH LÀM: 1 biến CSS `--chat-text-scale` đặt trên <html>, các class chữ
// bong bóng chat dùng `text-[calc(12.5px*var(--chat-text-scale,1))]` thay vì
// `text-[12.5px]` cố định (xem result/+page.svelte, history/+page.svelte) —
// Tailwind v4 cho phép arbitrary value là biểu thức CSS bất kỳ, không chỉ
// số cố định.
//
// Lưu localStorage — CHUNG cho mọi cửa sổ (main/result/history, cùng
// origin), nên đổi 1 lần ở Settings là mọi cửa sổ khác đồng bộ theo (cần tự
// apply lại ở mỗi cửa sổ lúc mount, xem +layout.svelte — localStorage không
// tự đồng bộ real-time giữa các cửa sổ đang mở sẵn, chỉ đọc đúng khi mở cửa
// sổ mới/reload, giống hệt cơ chế theme.ts).

export type TextSizeMode = "small" | "medium" | "large";

const STORAGE_KEY = "snip-ai:textSize";

const SCALE_BY_MODE: Record<TextSizeMode, string> = {
  small: "0.9",
  medium: "1",
  large: "1.25",
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

/** Áp dụng lên <html> của cửa sổ HIỆN TẠI — gọi lúc mount mọi route. */
export function applyTextSize(mode: TextSizeMode): void {
  document.documentElement.style.setProperty("--chat-text-scale", SCALE_BY_MODE[mode]);
}

export function setTextSize(mode: TextSizeMode): void {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // bỏ qua nếu không lưu được — vẫn apply cho phiên hiện tại
  }
  applyTextSize(mode);
}
