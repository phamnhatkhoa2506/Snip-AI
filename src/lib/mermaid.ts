// Vẽ sơ đồ (lưu đồ, sơ đồ tư duy, sequence, ERD, timeline...) khi AI trả lời
// bằng khối mã ```mermaid — AI TỰ QUYẾT khi nào câu hỏi cần 1 sơ đồ trực quan
// thay vì văn bản/danh sách thường (xem SYSTEM_PROMPT trong ai.rs, rule mới
// dặn AI dùng cú pháp Mermaid khi thực sự có ích), người dùng không cần bấm
// nút gì riêng — cứ hỏi/yêu cầu vẽ là AI tự quyết định có nên vẽ hay không.
//
// KHÔNG xử lý trong markdown.ts (nơi convert markdown -> HTML đồng bộ) —
// mermaid.render() là ASYNC (phải chạy layout thật bằng d3 mới ra được kích
// thước SVG), trong khi renderMarkdown() phải đồng bộ (gọi trực tiếp trong
// {@html ...}} của Svelte). Thay vào đó: markdown.ts cứ để khối ```mermaid
// hiện thành code block bình thường trước (marked tự escape an toàn), rồi
// action `mermaidBlocks` bên dưới quét DOM tìm các khối `code.language-mermaid`
// SAU KHI đã chèn vào trang, thay từng khối bằng SVG đã vẽ xong.
//
// Nặng (~kèm d3) nên KHÔNG import tĩnh ở đầu file — chỉ tải khi thực sự có
// ít nhất 1 khối mermaid cần vẽ (dynamic import), tránh làm chậm mọi cửa sổ
// "Kết quả AI"/Lịch sử kể cả khi câu trả lời không hề có sơ đồ nào.
import type { Action } from "svelte/action";

let mermaidPromise: ReturnType<typeof loadMermaid> | null = null;
let idSeq = 0;

function isDarkMode(): boolean {
  const attr = document.documentElement.getAttribute("data-theme");
  if (attr === "dark") return true;
  if (attr === "light") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

async function loadMermaid() {
  const mod = await import("mermaid");
  const mermaid = mod.default;
  mermaid.initialize({
    startOnLoad: false,
    // "strict": mermaid tự escape nhãn/nội dung, không cho nhúng HTML/script
    // qua nội dung sơ đồ — cần thiết vì nguồn sơ đồ đến từ AI (bán tin cậy),
    // giống lý do phải DOMPurify câu trả lời markdown thường.
    securityLevel: "strict",
    theme: isDarkMode() ? "dark" : "default",
  });
  return mermaid;
}

/** Dùng chung 1 promise load — nhiều khối mermaid trong cùng 1 câu trả lời
 * (hoặc nhiều cửa sổ) chỉ tải + khởi tạo thư viện đúng 1 lần. */
function ensureMermaid() {
  if (!mermaidPromise) mermaidPromise = loadMermaid();
  return mermaidPromise;
}

async function renderMermaidBlocks(container: HTMLElement): Promise<void> {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("code.language-mermaid"));
  if (codeEls.length === 0) return;

  const mermaid = await ensureMermaid();

  for (const codeEl of codeEls) {
    const pre = codeEl.closest("pre");
    // `data-mermaid-tried`: đã thử vẽ (thành công hoặc lỗi) — thành công thì
    // <pre> đã bị thay hẳn (không còn trong DOM để quét lại lần sau); lỗi thì
    // vẫn còn <pre> nhưng đã đánh dấu để KHÔNG thử vẽ lại vô ích mỗi lần
    // action `update()` được gọi lại (VD props cha đổi nhưng nội dung không đổi).
    if (!pre || pre.dataset.mermaidTried === "1") continue;
    pre.dataset.mermaidTried = "1";

    const source = codeEl.textContent ?? "";
    if (!source.trim()) continue;

    try {
      const id = `mermaid-diagram-${idSeq++}`;
      const { svg } = await mermaid.render(id, source);
      const wrapper = document.createElement("div");
      wrapper.className = "mermaid-diagram";
      wrapper.innerHTML = svg;
      pre.replaceWith(wrapper);
    } catch (e) {
      // Cú pháp mermaid AI sinh ra có thể sai (hallucinate) — KHÔNG để lỗi
      // này làm vỡ cả câu trả lời. Giữ nguyên khối code hiện tại (người dùng
      // vẫn đọc được ý định bằng cú pháp thô) thay vì mất trắng hoặc crash.
      console.warn("[snip-ai] Không vẽ được sơ đồ mermaid, giữ nguyên code gốc:", e);
    }
  }
}

/** Action gắn vào container `.markdown-body` (result/+page.svelte và
 * history/+page.svelte) — quét lại mỗi khi `dep` đổi giá trị (truyền
 * `turn.content` vào, Svelte tự gọi `update()` đúng lúc nội dung bong bóng
 * chat này đổi, không phải mọi lần re-render bất kỳ chỗ nào khác trong app). */
export const mermaidBlocks: Action<HTMLElement, unknown> = (node) => {
  renderMermaidBlocks(node);
  return {
    update() {
      renderMermaidBlocks(node);
    },
  };
};
