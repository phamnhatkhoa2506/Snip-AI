// Vẽ HÌNH HỌC (tam giác, đường tròn, góc, chứng minh...) khi AI trả lời bằng
// khối mã ```svg — khác hẳn Mermaid (chỉ vẽ sơ đồ logic/luồng, không vẽ được
// hình học thật) và khác khối ```plot (xem plot.ts — đồ thị hàm số CHÍNH XÁC
// tính bằng công thức). Hình học thuần tuý (tam giác ABC, đường tròn tâm O,
// góc...) KHÔNG có công thức đóng để tính-vẽ tự động như đồ thị hàm số — phải
// để AI tự mô tả hình. Thay vì bày ra 1 cú pháp riêng bắt AI học (rủi ro bịa
// sai cú pháp), tận dụng việc Gemini vốn đã rất quen thuộc với SVG chuẩn
// (gặp rất nhiều lúc train) — xem PROMPT_SVG_FIGURE/rule liên quan trong
// ai.rs. ĐÁNH ĐỔI cần chấp nhận: hình vẽ tay kiểu này KHÔNG đảm bảo tỉ lệ/số
// đo chuẩn tuyệt đối như ```plot — chỉ đủ MINH HOẠ, số liệu thật phải theo lời
// văn/LaTeX, không phải đo trên hình.
//
// Khác Mermaid (mermaid.render() tự escape nội dung, "securityLevel: strict"
// đã đủ an toàn) — SVG ở đây do AI viết TRỰC TIẾP, phải tự sanitize bằng
// DOMPurify (hồ sơ SVG) trước khi chèn vào DOM qua innerHTML, đề phòng AI bị
// dẫn dụ nhúng script/sự kiện độc hại vào bên trong (prompt injection từ nội
// dung ảnh/video/tài liệu đính kèm — xem thêm ghi chú DOMPurify trong
// markdown.ts).
import type { Action } from "svelte/action";
import DOMPurify from "dompurify";
import { openSvgZoomModal } from "./mermaid";
import { exportSvgAsPng } from "./exportImage";

let idSeq = 0;

/** Sanitize an toàn rồi trả về markup SVG đã làm sạch, hoặc `null` nếu sau
 * khi sanitize không còn gì (VD AI lỡ gửi rỗng/toàn thẻ bị chặn). `USE_PROFILES:
 * {svg: true, svgFilters: true}` — hồ sơ SVG riêng của DOMPurify (khác hồ sơ
 * HTML mặc định dùng cho renderMarkdown), cho phép đúng tập thẻ/thuộc tính SVG
 * chuẩn (path, circle, line, polygon, text, defs, marker...) và filter, vẫn
 * chặn `<script>`/`on*`/`xlink:href` trỏ ra ngoài như HTML thường. */
function sanitizeSvg(source: string): string | null {
  const clean = DOMPurify.sanitize(source, {
    USE_PROFILES: { svg: true, svgFilters: true },
  }).trim();
  if (!clean || !/<svg[\s>]/i.test(clean)) return null;
  return clean;
}

async function renderSvgFigureBlocks(container: HTMLElement): Promise<void> {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("code.language-svg"));
  if (codeEls.length === 0) return;

  for (const codeEl of codeEls) {
    const pre = codeEl.closest("pre");
    // `data-svg-figure-tried`: cùng cơ chế `data-mermaid-tried`/`data-plot-tried`
    // — đã thử vẽ thì không thử lại vô ích mỗi lần action `update()` chạy lại.
    if (!pre || pre.dataset.svgFigureTried === "1") continue;
    pre.dataset.svgFigureTried = "1";

    const source = codeEl.textContent ?? "";
    if (!source.trim()) continue;

    const clean = sanitizeSvg(source);
    // Sanitize xong không còn gì hợp lệ (AI trả lung tung, không phải SVG
    // thật) — giữ nguyên khối code gốc, không vỡ cả câu trả lời.
    if (!clean) continue;

    const wrapper = document.createElement("div");
    wrapper.className = "svg-figure";
    // Nền TRẮNG CỐ ĐỊNH (không theo `var(--color-bg-elevated)` đổi theo theme
    // app nữa) — AI được dặn vẽ hình giả định nền trắng (xem PROMPT_SVG rule
    // trong ai.rs, nét/chữ màu tối), nên phải LUÔN thật sự là nền trắng, kể cả
    // khi app đang bật giao diện tối, không thì hình vẽ ra sẽ "chìm" vào nền
    // tối của bong bóng chat.
    wrapper.style.cssText =
      "position:relative;display:inline-block;max-width:100%;border-radius:10px;overflow:hidden;" +
      "background:#fff;border:1px solid #e2e2e2;padding:8px;";
    wrapper.innerHTML = clean;

    const svgEl = wrapper.querySelector("svg");
    if (svgEl) {
      svgEl.id = `svg-figure-${idSeq++}`;
      // AI thường không tự set kích thước hợp lý (hoặc set quá lớn/quá nhỏ) —
      // ép hiển thị vừa khung bong bóng chat, giữ đúng tỉ lệ qua viewBox nếu
      // có sẵn; nếu AI quên viewBox thì đành hiện đúng width/height AI cho.
      svgEl.style.display = "block";
      svgEl.style.width = "100%";
      svgEl.style.maxWidth = "360px";
      svgEl.style.height = "auto";
    }

    const zoomBtn = document.createElement("button");
    zoomBtn.type = "button";
    zoomBtn.title = "Phóng to hình (cuộn chuột để zoom, kéo để di chuyển)";
    zoomBtn.setAttribute("aria-label", "Phóng to hình");
    zoomBtn.innerHTML = ZOOM_ICON_SVG;
    zoomBtn.style.cssText =
      "position:absolute;top:6px;right:6px;width:26px;height:26px;border-radius:6px;" +
      "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
      "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
      "justify-content:center;padding:0;";
    zoomBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      openSvgZoomModal(clean);
    });
    wrapper.appendChild(zoomBtn);

    if (svgEl) {
      const downloadBtn = document.createElement("button");
      downloadBtn.type = "button";
      downloadBtn.title = "Tải ảnh PNG";
      downloadBtn.setAttribute("aria-label", "Tải ảnh PNG");
      downloadBtn.innerHTML = DOWNLOAD_ICON_SVG;
      downloadBtn.style.cssText =
        "position:absolute;top:34px;right:6px;width:26px;height:26px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
        "justify-content:center;padding:0;";
      downloadBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        exportSvgAsPng(svgEl as unknown as SVGSVGElement, "hinh-ve.png").catch((err) =>
          console.warn("[snip-ai] Xuất PNG hình vẽ thất bại:", err),
        );
      });
      wrapper.appendChild(downloadBtn);
    }

    pre.replaceWith(wrapper);
  }
}

const ZOOM_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/></svg>';

const DOWNLOAD_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg>';

/** Action gắn vào container `.markdown-body` (result/+page.svelte và
 * history/+page.svelte) — cùng cách dùng với `mermaidBlocks`/`plotBlocks`. */
export const svgFigureBlocks: Action<HTMLElement, unknown> = (node) => {
  renderSvgFigureBlocks(node);
  return {
    update() {
      renderSvgFigureBlocks(node);
    },
  };
};
