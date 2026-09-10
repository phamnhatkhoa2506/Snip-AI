// Phím tắt chụp màn hình tuỳ chỉnh — logic đăng ký/lưu thật nằm ở Rust
// (src-tauri/src/hotkey.rs). File này chỉ lo phần "ghi lại tổ hợp phím người
// dùng vừa nhấn" và đổi sang định dạng accelerator mà Rust hiểu được
// (VD "Ctrl+Alt+S", "Ctrl+PrintScreen").

import { invoke } from "@tauri-apps/api/core";

export async function getHotkey(): Promise<string> {
  return invoke<string>("get_hotkey");
}

/** Đổi phím tắt; trả về accelerator đã chuẩn hoá do Rust format lại (để hiển
 * thị đúng dạng mà lệnh Rust hiểu, tránh lệch chính tả). */
export async function setHotkey(accelerator: string): Promise<string> {
  return invoke<string>("set_hotkey", { accelerator });
}

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
]);

// Map từ KeyboardEvent.code (ổn định theo vị trí phím vật lý, không đổi theo
// layout bàn phím) sang tên phím dạng "accelerator" mà Tauri hiểu.
const CODE_TO_KEY: Record<string, string> = {
  PrintScreen: "PrintScreen",
  Escape: "Escape",
  Space: "Space",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Enter: "Enter",
  NumpadEnter: "Enter",
  // Phím dấu câu/ký hiệu — trước đây thiếu, khiến người dùng bấm thử 1 tổ hợp
  // có phím này làm phím chính thì bộ ghi phím không nhận ra gì cả (cảm giác
  // như "không bấm được"), dù về mặt hệ điều hành các phím này hoàn toàn dùng
  // được cho global hotkey. Tên bên phải trùng với tên `Code` mà crate
  // `global-shortcut` (Rust) hiểu, nên giữ nguyên không cần đổi tên.
  Comma: "Comma",
  Period: "Period",
  Semicolon: "Semicolon",
  Quote: "Quote",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Backslash: "Backslash",
  Minus: "Minus",
  Equal: "Equal",
  Backquote: "Backquote",
  Slash: "Slash",
};
for (let i = 1; i <= 24; i++) CODE_TO_KEY[`F${i}`] = `F${i}`;
for (let i = 0; i < 26; i++) {
  const letter = String.fromCharCode(65 + i);
  CODE_TO_KEY[`Key${letter}`] = letter;
}
for (let i = 0; i <= 9; i++) CODE_TO_KEY[`Digit${i}`] = String(i);

/** Chuẩn hoá 1 phần hiển thị (VD "KeyH" -> "H", "Digit5" -> "5") — cần dùng
 * khi tách accelerator string NHẬN TỪ RUST (`get_hotkey`/`set_hotkey` trả về
 * `Shortcut::to_string()`, định dạng nội bộ của crate `global-shortcut`,
 * khác với tên hiển thị thân thiện). Combo tự bắt qua `captureCombo()` ở
 * trên đã map sẵn qua `CODE_TO_KEY` nên KHÔNG cần qua hàm này lần nữa. */
export function formatKeyLabel(part: string): string {
  const keyMatch = /^Key([A-Z])$/.exec(part);
  if (keyMatch) return keyMatch[1];
  const digitMatch = /^Digit(\d)$/.exec(part);
  if (digitMatch) return digitMatch[1];
  return part;
}

export interface CapturedCombo {
  accelerator: string;
  /** Các phần hiển thị (VD ["Ctrl", "Alt", "S"]) để render dạng kbd đẹp */
  parts: string[];
}

/** Đọc 1 sự kiện keydown, trả về tổ hợp phím hoàn chỉnh nếu đủ điều kiện:
 * - Phải có ít nhất 1 phím bổ trợ (Ctrl/Alt/Shift/Win) — tránh lỡ tay chiếm
 *   mất 1 phím thường đang dùng để gõ chữ ở chỗ khác.
 * - Phím chính phải nằm trong danh sách nhận diện được (chữ, số, F1-F24,
 *   PrintScreen, phím điều hướng...).
 * Trả về `null` nếu chưa đủ (VD người dùng mới bấm mỗi phím Ctrl). */
export function captureCombo(e: KeyboardEvent): CapturedCombo | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const key = CODE_TO_KEY[e.code];
  if (!key) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  if (parts.length === 0) return null;

  parts.push(key);
  return { accelerator: parts.join("+"), parts };
}
