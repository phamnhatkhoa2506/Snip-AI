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
      wrapper.style.position = "relative";
      wrapper.innerHTML = svg;

      // Sơ đồ phức tạp (nhiều node) hiện nhúng trong bong bóng chat quá nhỏ
      // để đọc — nút phóng to mở modal riêng, cho zoom/pan thoải mái (xem
      // openMermaidZoomModal). Luôn hiện sẵn (không đợi hover) — cùng nếp với
      // nút phóng to của "Sơ đồ từ vựng" (VocabDiagram.svelte).
      const zoomBtn = document.createElement("button");
      zoomBtn.type = "button";
      zoomBtn.title = "Phóng to sơ đồ (cuộn chuột để zoom, kéo để di chuyển)";
      zoomBtn.setAttribute("aria-label", "Phóng to sơ đồ");
      zoomBtn.innerHTML = ZOOM_ICON_SVG;
      zoomBtn.style.cssText =
        "position:absolute;top:6px;right:6px;width:26px;height:26px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
        "justify-content:center;padding:0;";
      zoomBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openMermaidZoomModal(svg);
      });
      wrapper.appendChild(zoomBtn);

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

// ── Phóng to/thu nhỏ sơ đồ (modal riêng, kéo để di chuyển) ─────────────────
// Dựng THẲNG bằng DOM API (không phải component Svelte) — `mermaid.ts` vốn
// đã thao tác DOM trực tiếp (renderMermaidBlocks ở trên), tự chứa nốt modal
// này tại đây tránh phải "bắn" state ngược lên `result/+page.svelte` VÀ
// `history/+page.svelte` (2 nơi dùng chung file này) chỉ để mở 1 modal —
// đơn giản hơn nhiều so với việc nối dây state qua lại giữa 2 component cha
// khác nhau. Style viết INLINE (không dùng class Tailwind) vì phần tử này
// KHÔNG nằm trong 1 file .svelte nào để Tailwind quét thấy lúc build.

const ZOOM_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/></svg>';

const MIN_ZOOM = 0.15;
const MAX_ZOOM = 8;

function openMermaidZoomModal(svgMarkup: string): void {
  const overlay = document.createElement("div");
  overlay.style.cssText =
    "position:fixed;inset:0;z-index:9999;background:rgba(0,0,0,.82);" +
    "display:flex;align-items:center;justify-content:center;overflow:hidden;" +
    "cursor:grab;touch-action:none;";

  const stage = document.createElement("div");
  stage.style.cssText = "position:absolute;top:0;left:0;";
  stage.innerHTML = svgMarkup;
  const svgEl = stage.querySelector("svg");
  if (svgEl) {
    // Bỏ mọi ràng buộc kích thước cũ (nếu có) — muốn kích thước THẬT của sơ
    // đồ để tính zoom "vừa khung" chính xác, không bị CSS bên ngoài ảnh hưởng.
    svgEl.style.display = "block";
    svgEl.style.maxWidth = "none";
    svgEl.style.maxHeight = "none";
  }
  overlay.appendChild(stage);

  let scale = 1;
  let x = 0;
  let y = 0;
  // Kích thước THẬT của sơ đồ ở scale=1 (đo 1 LẦN, xem fitToView) — phóng to
  // bằng cách đặt lại width/height THẬT của thẻ <svg> (ép trình duyệt vẽ lại
  // vector ở đúng độ phân giải mới), KHÔNG dùng `transform: scale()`. Lý do:
  // scale() qua CSS transform + `will-change` dễ khiến trình duyệt CACHE 1
  // bitmap đã "chụp" sẵn ở kích thước ban đầu rồi chỉ phóng to TẤM BITMAP đó
  // lên — sơ đồ (vốn là vector, đáng lẽ luôn nét dù zoom cỡ nào) bị MỜ hẳn đi
  // giống phóng to 1 tấm ảnh raster (lỗi thực tế đã gặp). Resize width/height
  // thật thì trình duyệt buộc phải render lại từ đầu, luôn nét.
  let baseWidth = 0;
  let baseHeight = 0;

  function applyTransform() {
    stage.style.transform = `translate(${x}px, ${y}px)`;
    if (svgEl && baseWidth > 0 && baseHeight > 0) {
      svgEl.style.width = `${baseWidth * scale}px`;
      svgEl.style.height = `${baseHeight * scale}px`;
    }
  }

  /** Co/giãn cho vừa khung nhìn lần đầu mở — sơ đồ nhỏ thì hiện đúng size
   * thật (không phóng to vô nghĩa), sơ đồ lớn hơn khung thì thu nhỏ vừa đủ
   * để thấy toàn cảnh trước, sau đó người dùng tự zoom sâu vào phần cần xem. */
  function fitToView() {
    if (!svgEl) return;
    if (baseWidth === 0 || baseHeight === 0) {
      // Đo kích thước GỐC (chưa co giãn gì) đúng 1 lần duy nhất — mọi lần
      // fitToView/zoom SAU đó đều tính lại TỪ con số gốc này, không đo lại
      // qua getBoundingClientRect() (lúc đó đã bị style width/height của
      // chính ta áp vào, đo lại sẽ ra kích thước ĐÃ SCALE chứ không phải gốc).
      const rect0 = svgEl.getBoundingClientRect();
      baseWidth = rect0.width;
      baseHeight = rect0.height;
    }
    if (baseWidth === 0 || baseHeight === 0) return;
    const overlayRect = overlay.getBoundingClientRect();
    const fit = Math.min(1, (overlayRect.width - 80) / baseWidth, (overlayRect.height - 80) / baseHeight);
    scale = Number.isFinite(fit) && fit > 0 ? fit : 1;
    x = (overlayRect.width - baseWidth * scale) / 2;
    y = (overlayRect.height - baseHeight * scale) / 2;
    applyTransform();
  }

  function zoomBy(factor: number, centerX: number, centerY: number) {
    const prevScale = scale;
    scale = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, scale * factor));
    // Giữ đúng điểm đang zoom (centerX/Y) đứng yên trên màn hình thay vì để
    // sơ đồ "nhảy" lệch tâm mỗi lần zoom — kỹ thuật kinh điển: quy điểm đó về
    // toạ độ TRONG sơ đồ (chưa scale) trước, rồi tính lại vị trí theo scale mới.
    x = centerX - ((centerX - x) / prevScale) * scale;
    y = centerY - ((centerY - y) / prevScale) * scale;
    applyTransform();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = overlay.getBoundingClientRect();
    zoomBy(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - rect.left, e.clientY - rect.top);
  }

  let dragging = false;
  let lastX = 0;
  let lastY = 0;
  // Bấm-thả TẠI CHỖ (không di chuyển) trên nền tối vẫn phát sinh 1 sự kiện
  // "click" bình thường (đúng ý muốn: đóng modal) — nhưng NẾU đã có kéo
  // (dù bắt đầu từ nền tối, không phải từ sơ đồ) thì "click" đó KHÔNG được
  // tính là "bấm ra ngoài để đóng" nữa, chỉ là điểm kết thúc của thao tác
  // kéo. Thiếu cờ này thì kéo qua lại vài lần là modal tự đóng ngang xương
  // (lỗi thực tế đã gặp) — trình duyệt vẫn bắn "click" sau "pointerup" dù
  // đã di chuyển, miễn đích bắt đầu/kết thúc trùng 1 phần tử.
  let didDrag = false;
  function onPointerDown(e: PointerEvent) {
    // Bấm vào 1 nút (thanh công cụ +/-/vừa khung/đóng) — TUYỆT ĐỐI không bắt
    // đầu kéo/chiếm pointer ở đây. `setPointerCapture` trên `overlay` khiến
    // trình duyệt tính lại ĐÍCH của sự kiện "click" phát sinh sau đó thành
    // chính `overlay` (không phải nút vừa bấm) — trùng khớp điều kiện "bấm
    // ra ngoài thì đóng" (`onOverlayClick`), làm modal tự đóng oan ngay khi
    // bấm bất kỳ nút nào trong thanh công cụ (lỗi thực tế đã gặp).
    if ((e.target as HTMLElement).closest("button")) return;
    dragging = true;
    didDrag = false;
    lastX = e.clientX;
    lastY = e.clientY;
    overlay.style.cursor = "grabbing";
    overlay.setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    // Ngưỡng nhỏ (3px) — phân biệt "kéo thật" với rung tay lúc bấm/thả tại
    // chỗ (chuột không bao giờ đứng yên tuyệt đối 100%).
    if (Math.abs(dx) > 3 || Math.abs(dy) > 3) didDrag = true;
    x += dx;
    y += dy;
    lastX = e.clientX;
    lastY = e.clientY;
    applyTransform();
  }
  function onPointerUp() {
    dragging = false;
    overlay.style.cursor = "grab";
  }
  function onDoubleClick() {
    fitToView();
  }
  function close() {
    document.removeEventListener("keydown", onKeydown);
    overlay.remove();
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
  function onOverlayClick(e: MouseEvent) {
    // Vừa kéo xong (dù thả tay trên nền tối) — KHÔNG tính là "bấm ra ngoài
    // để đóng", chỉ là điểm kết thúc thao tác kéo. Xem giải thích ở `didDrag`.
    if (didDrag) {
      didDrag = false;
      return;
    }
    // Chỉ đóng khi bấm ĐÚNG vào nền tối (backdrop) — `stage` chỉ chiếm đúng
    // khung sơ đồ (không phủ hết overlay), nên click ra ngoài sơ đồ luôn có
    // `e.target === overlay`, không cần chặn nổi bọt event thủ công.
    if (e.target === overlay) close();
  }

  overlay.addEventListener("wheel", onWheel, { passive: false });
  overlay.addEventListener("pointerdown", onPointerDown);
  overlay.addEventListener("pointermove", onPointerMove);
  overlay.addEventListener("pointerup", onPointerUp);
  overlay.addEventListener("dblclick", onDoubleClick);
  overlay.addEventListener("click", onOverlayClick);
  document.addEventListener("keydown", onKeydown);

  // Thanh nút góc trên-phải: phóng to/thu nhỏ/vừa khung/đóng — cho người
  // không quen cuộn chuột/kéo thả vẫn dùng được đầy đủ.
  const toolbar = document.createElement("div");
  toolbar.style.cssText = "position:absolute;top:16px;right:16px;display:flex;gap:8px;z-index:1;";
  function makeToolbarButton(label: string, title: string, onClick: () => void): HTMLButtonElement {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.title = title;
    btn.setAttribute("aria-label", title);
    btn.textContent = label;
    btn.style.cssText =
      "width:34px;height:34px;border-radius:8px;border:1px solid rgba(255,255,255,.25);" +
      "background:rgba(255,255,255,.1);color:#fff;font-size:17px;line-height:1;cursor:pointer;" +
      "display:flex;align-items:center;justify-content:center;";
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      onClick();
    });
    return btn;
  }
  const centerOfOverlay = (): [number, number] => {
    const rect = overlay.getBoundingClientRect();
    return [rect.width / 2, rect.height / 2];
  };
  toolbar.appendChild(
    makeToolbarButton("+", "Phóng to", () => {
      const [cx, cy] = centerOfOverlay();
      zoomBy(1.25, cx, cy);
    }),
  );
  toolbar.appendChild(
    makeToolbarButton("–", "Thu nhỏ", () => {
      const [cx, cy] = centerOfOverlay();
      zoomBy(1 / 1.25, cx, cy);
    }),
  );
  toolbar.appendChild(makeToolbarButton("⤢", "Vừa khung", fitToView));
  toolbar.appendChild(makeToolbarButton("×", "Đóng (Esc)", close));
  overlay.appendChild(toolbar);

  document.body.appendChild(overlay);
  // Đợi 1 khung hình để trình duyệt tính xong layout thật của SVG vừa chèn
  // (getBoundingClientRect() ngay lúc mới appendChild có thể chưa chính xác).
  requestAnimationFrame(fitToView);
}
