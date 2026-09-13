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
// origin) NHƯNG localStorage KHÔNG tự đẩy thay đổi real-time giữa các cửa sổ
// ĐANG MỞ SẴN (chỉ đọc đúng khi mở cửa sổ mới/reload) — lỗi thực tế đã gặp:
// đổi cỡ chữ ở Settings trong lúc cửa sổ "Kết quả AI" đang mở sẵn thì chữ ở
// đó không đổi gì cả, trông như nút "không ăn". Sửa bằng cách BẮN THÊM 1 sự
// kiện Tauri (`emit`, không phải `emitTo` — cần tới MỌI cửa sổ đang mở, không
// chỉ 1 cửa sổ cụ thể) ngay khi đổi, các cửa sổ khác lắng nghe rồi tự áp
// dụng lại NGAY, không cần đóng/mở lại (xem +layout.svelte).

import { emit } from "@tauri-apps/api/event";

export type TextSizeMode = "small" | "medium" | "large";

/** Tên sự kiện Tauri dùng để đồng bộ real-time giữa các cửa sổ đang mở. */
export const TEXT_SIZE_CHANGED_EVENT = "snip-ai:text-size-changed";

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

export async function setTextSize(mode: TextSizeMode): Promise<void> {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // bỏ qua nếu không lưu được — vẫn apply cho phiên hiện tại
  }
  applyTextSize(mode);
  try {
    // `emit` (không phải `emitTo`) — cần tới MỌI cửa sổ đang mở (result-*,
    // history...), không biết trước có bao nhiêu cửa sổ hay label gì.
    await emit(TEXT_SIZE_CHANGED_EVENT, mode);
  } catch (e) {
    console.warn("[snip-ai] Không đồng bộ được cỡ chữ real-time tới cửa sổ khác:", e);
  }
}
