// Nút "Chép" cho TỪNG khối mã (```code```) trong câu trả lời AI — trước đây
// chỉ có nút "Chép" cho CẢ câu trả lời (xem handleCopyTurn ở result/
// +page.svelte), không tiện khi chỉ cần lấy đúng 1 đoạn code, phải chép cả
// câu trả lời rồi tự cắt phần cần dùng ra.
//
// CHỈ áp dụng cho khối mã THƯỜNG (Python/JS/log/...) — KHÔNG áp dụng cho các
// khối mã ĐẶC BIỆT đã có xử lý riêng (```mermaid/```plot/```svg/```chart/
// ```csv/```3d — xem mermaid.ts/plot.ts/svgFigure.ts/chart.ts/csvBlock.ts/
// scene3d.ts): mỗi khối đó tự THAY HẲN <pre> bằng sơ đồ/biểu đồ/hình vẽ thật,
// "chép nguyên văn code" không còn ý nghĩa gì với chúng nữa. Nhận diện qua
// đúng class "language-xxx" marked gắn sẵn — không cần biết action nào xử lý
// ngôn ngữ nào, chỉ cần liệt kê tên đã dùng.
//
// Cùng pattern quét DOM sau khi markdown.ts render xong (xem mermaid.ts) —
// nhưng KHÔNG cần dynamic import gì (chỉ thao tác DOM + clipboard), nên chạy
// ĐỒNG BỘ, không async như các action kia.
import type { Action } from "svelte/action";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

const SPECIAL_LANGUAGES = new Set(["mermaid", "plot", "svg", "chart", "csv", "3d"]);

const COPY_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<rect x="8" y="8" width="14" height="14" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>';

const CHECK_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M20 6 9 17l-5-5"/></svg>';

function renderCodeCopyButtons(container: HTMLElement): void {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("pre > code"));
  for (const codeEl of codeEls) {
    const pre = codeEl.parentElement as HTMLElement | null;
    // `data-copy-btn-added`: đã thêm nút rồi thì thôi — tránh thêm chồng nút
    // mỗi lần action `update()` chạy lại (VD props cha đổi nhưng nội dung
    // không đổi). Nếu `pre` này thuộc 1 khối ĐẶC BIỆT thì nó sẽ bị action
    // riêng THAY HẲN khỏi DOM sau đó — không cần lo dọn dẹp gì thêm ở đây.
    if (!pre || pre.dataset.copyBtnAdded === "1") continue;

    // Class thật marked gắn là "language-xxx" — bóc đúng tên "xxx" để so với
    // danh sách đặc biệt ở trên.
    const langClass = Array.from(codeEl.classList).find((c) => c.startsWith("language-"));
    const lang = langClass?.slice("language-".length);
    if (lang && SPECIAL_LANGUAGES.has(lang)) continue; // để dành cho action riêng xử lý khối này

    pre.dataset.copyBtnAdded = "1";
    // `.markdown-body pre` vốn đã `overflow-x: auto` để cuộn ngang dòng dài
    // (xem app.css) — containing block của phần tử `position:absolute` bên
    // trong LUÔN là padding box của `pre` (không cuộn theo nội dung), nên nút
    // vẫn đứng yên đúng góc trên-phải dù cuộn ngang tới đâu, không cần xử lý
    // gì thêm ngoài `position: relative` ở đây.
    pre.style.position = "relative";

    const btn = document.createElement("button");
    btn.type = "button";
    btn.title = "Chép đoạn mã này";
    btn.setAttribute("aria-label", "Chép đoạn mã này");
    btn.innerHTML = COPY_ICON_SVG;
    btn.style.cssText =
      "position:absolute;top:6px;right:6px;width:24px;height:24px;border-radius:6px;" +
      "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
      "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
      "justify-content:center;padding:0;opacity:0.85;";
    btn.addEventListener("mouseenter", () => (btn.style.opacity = "1"));
    btn.addEventListener("mouseleave", () => (btn.style.opacity = "0.85"));
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      // `codeEl.textContent` — LUÔN đúng ký tự gốc dù sau này có thêm tô màu
      // cú pháp (chèn thêm thẻ <span> con), khác `innerHTML` dễ dính thẻ.
      writeText(codeEl.textContent ?? "")
        .then(() => {
          btn.innerHTML = CHECK_ICON_SVG;
          btn.style.color = "var(--color-accent)";
          setTimeout(() => {
            btn.innerHTML = COPY_ICON_SVG;
            btn.style.color = "var(--color-text-muted)";
          }, 1400);
        })
        .catch((err) => console.warn("[snip-ai] Chép đoạn mã thất bại:", err));
    });
    pre.appendChild(btn);
  }
}

/** Action gắn vào container `.markdown-body` (result/+page.svelte và
 * history/+page.svelte) — cùng cách dùng với `mermaidBlocks`/`plotBlocks`. */
export const codeCopyButtons: Action<HTMLElement, unknown> = (node) => {
  renderCodeCopyButtons(node);
  return {
    update() {
      renderCodeCopyButtons(node);
    },
  };
};
