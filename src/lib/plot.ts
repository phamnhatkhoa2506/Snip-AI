// Vẽ ĐỒ THỊ HÀM SỐ khi AI trả lời bằng khối mã ```plot — khác hẳn Mermaid
// (chỉ vẽ được sơ đồ logic/luồng, KHÔNG vẽ được đồ thị hàm số ĐÚNG TỈ LỆ) và
// khác khối ```svg (xem svgFigure.ts — hình học AI tự "vẽ tay" bằng SVG,
// không đảm bảo chuẩn số đo). Ở ĐÂY, AI CHỈ cần khai báo CÔNG THỨC + miền giá
// trị dạng JSON, việc TÍNH TOÁN VÀ VẼ CHÍNH XÁC (không đoán/hoạ toạ độ) giao
// hẳn cho `function-plot` (dựng trên d3) — xem rule mới trong SYSTEM_PROMPT
// (ai.rs) dặn AI dùng khối này cho đồ thị hàm số/khảo sát hàm.
//
// Cùng pattern với mermaid.ts: markdown.ts để khối ```plot hiện thành code
// block bình thường trước (marked tự escape nội dung JSON an toàn), action
// `plotBlocks` bên dưới quét DOM SAU KHI đã chèn vào trang, thay bằng chart
// thật. Nặng (kèm d3) nên dynamic import, không tải nếu câu trả lời không có
// khối ```plot nào.
import type { Action } from "svelte/action";
import type { FunctionPlotDatum, FunctionPlotOptions } from "function-plot";
import { exportSvgAsPng } from "./exportImage";

let functionPlotPromise: ReturnType<typeof loadFunctionPlot> | null = null;

/** Kiểu hàm thật sự (`options => Chart`) — tách riêng type này vì bên dưới
 * phải tự dò đúng chỗ hàm nằm (xem giải thích), không thể chỉ viết
 * `typeof import("function-plot").default` như bình thường. */
type FunctionPlotFn = (options: FunctionPlotOptions) => unknown;

/** LỖI THỰC TẾ đã gặp: gọi thẳng `(await import("function-plot")).default(...)`
 * ra "TypeError: functionPlot is not a function". Nguyên nhân: `function-plot`
 * là gói CommonJS kiểu cũ (`exports.default = functionPlot`, gắn thêm cả
 * `exports.globals`/`exports.$eval`... bằng `Object.defineProperty` thay vì
 * gán thẳng) — kiểu export "lai" này khiến `cjs-module-lexer` (Vite dùng để
 * dò named export tĩnh khi pre-bundle dep) KHÔNG phân tích được, nên bản Vite
 * build ra chỉ có ĐÚNG 1 dòng `export default require_dist();` — nghĩa là
 * `import("function-plot")` trả về `{ default: <TOÀN BỘ object exports CJS
 * gốc> }`, không phải `{ default: <hàm functionPlot> }` như import ESM bình
 * thường. Hàm thật nằm sâu thêm 1 tầng nữa: `mod.default.default`. Dò cả 2
 * khả năng (`mod.default` HOẶC `mod.default.default`) cho chắc — không phụ
 * thuộc hành vi bundling cụ thể của 1 phiên bản Vite nào, phòng trường hợp
 * cách "bóc lớp" này đổi khác ở bản Vite sau. */
async function loadFunctionPlot(): Promise<FunctionPlotFn> {
  const mod = (await import("function-plot")) as unknown as {
    default: FunctionPlotFn | { default: FunctionPlotFn };
  };
  const candidate = mod.default;
  const fn = typeof candidate === "function" ? candidate : candidate?.default;
  if (typeof fn !== "function") {
    throw new Error("Không tải được thư viện vẽ đồ thị (function-plot) đúng cách");
  }
  return fn;
}

/** Dùng chung 1 promise load — nhiều khối ```plot trong cùng 1 câu trả lời
 * (hoặc nhiều cửa sổ) chỉ tải thư viện đúng 1 lần. */
function ensureFunctionPlot() {
  if (!functionPlotPromise) functionPlotPromise = loadFunctionPlot();
  return functionPlotPromise;
}

/** Cú pháp JSON đơn giản AI phải theo (xem PROMPT_PLOT trong ai.rs) — CHỈ
 * khai báo công thức/miền giá trị/điểm đáng chú ý bằng SỐ THẬT, KHÔNG tự tính
 * toạ độ pixel hay tự vẽ path gì cả — function-plot lo hết phần vẽ chính xác
 * từ công thức, loại bỏ hẳn rủi ro AI "hoạ" sai tỉ lệ/điểm cực trị/giao điểm. */
interface PlotSpec {
  title?: string;
  xDomain?: [number, number];
  yDomain?: [number, number];
  functions?: { fn: string; color?: string; label?: string }[];
  points?: { x: number; y: number; label?: string }[];
}

function parsePlotSpec(source: string): PlotSpec | null {
  try {
    const spec = JSON.parse(source);
    if (!spec || typeof spec !== "object" || Array.isArray(spec)) return null;
    return spec as PlotSpec;
  } catch {
    return null;
  }
}

/** Bảng màu cố định (khớp tông accent của app) cho nhiều hàm cùng lúc trên 1
 * đồ thị (VD so sánh 2-3 hàm) — AI có thể tự chỉ định `color` riêng, đây chỉ
 * là màu mặc định khi AI không chỉ định. */
const DEFAULT_COLORS = ["#7c5cff", "#22c55e", "#f59e0b", "#ef4444", "#06b6d4"];

function buildOptions(spec: PlotSpec, target: HTMLElement, width: number, height: number): FunctionPlotOptions {
  const data: FunctionPlotDatum[] = (spec.functions ?? [])
    .filter((f) => typeof f.fn === "string" && f.fn.trim())
    .map((f, i) => ({ fn: f.fn, color: f.color || DEFAULT_COLORS[i % DEFAULT_COLORS.length] }));

  const points = (spec.points ?? []).filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
  if (points.length > 0) {
    data.push({
      points: points.map((p) => [p.x, p.y]),
      fnType: "points",
      graphType: "scatter",
      color: "#ef4444",
      skipTip: true,
    });
    // Nhãn từng điểm (nếu có) là 1 datum "text" RIÊNG mỗi điểm — function-plot
    // không hỗ trợ gắn nhãn kèm theo scatter, phải khai báo tách rời.
    for (const p of points) {
      if (!p.label) continue;
      data.push({
        graphType: "text",
        fnType: "points",
        location: [p.x, p.y],
        text: p.label,
        skipTip: true,
      } as FunctionPlotDatum);
    }
  }

  return {
    target,
    width,
    height,
    title: spec.title,
    grid: true,
    xAxis: spec.xDomain ? { domain: spec.xDomain } : undefined,
    yAxis: spec.yDomain ? { domain: spec.yDomain } : undefined,
    data,
  };
}

async function renderPlotBlocks(container: HTMLElement): Promise<void> {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("code.language-plot"));
  if (codeEls.length === 0) return;

  const functionPlot = await ensureFunctionPlot();

  for (const codeEl of codeEls) {
    const pre = codeEl.closest("pre");
    // `data-plot-tried`: giống hệt `data-mermaid-tried` ở mermaid.ts — đã thử
    // vẽ (thành công hoặc lỗi) thì không thử lại vô ích mỗi lần action
    // `update()` được gọi lại.
    if (!pre || pre.dataset.plotTried === "1") continue;
    pre.dataset.plotTried = "1";

    const source = codeEl.textContent ?? "";
    const spec = parsePlotSpec(source);
    // JSON hỏng (AI hallucinate cú pháp) — giữ nguyên khối code gốc, người
    // dùng vẫn đọc được ý định bằng JSON thô, không vỡ cả câu trả lời.
    if (!spec) continue;

    try {
      const wrapper = document.createElement("div");
      wrapper.className = "function-plot-chart";
      // Nền TRẮNG CỐ ĐỊNH (không theo `var(--color-bg-elevated)` đổi theo
      // theme app nữa) — trục/lưới/nhãn của function-plot vẽ màu tối theo
      // mặc định của thư viện, cần nền sáng cố định để luôn tương phản rõ dù
      // app đang ở giao diện tối hay sáng. Viền cũng cố định màu xám nhạt
      // thay vì `var(--color-border)` (có thể là màu tối trong theme tối,
      // lệch tông với nền trắng bên trong).
      wrapper.style.cssText =
        "position:relative;border-radius:10px;overflow:hidden;background:#fff;" +
        "border:1px solid #e2e2e2;display:inline-block;max-width:100%;";

      const chartEl = document.createElement("div");
      // Rộng theo đúng khung bong bóng chat đang có (không tràn ra ngoài) —
      // giới hạn 420px cho gọn, đủ đọc rõ đồ thị đơn giản; đồ thị phức tạp
      // hơn thì bấm nút phóng to xem bản lớn (xem openPlotZoomModal).
      const width = Math.max(240, Math.min(420, container.clientWidth || 420));
      chartEl.style.cssText = `width:${width}px;max-width:100%;`;
      wrapper.appendChild(chartEl);
      functionPlot(buildOptions(spec, chartEl, width, 260));

      const zoomBtn = document.createElement("button");
      zoomBtn.type = "button";
      zoomBtn.title = "Phóng to đồ thị (cuộn chuột để zoom, kéo để di chuyển)";
      zoomBtn.setAttribute("aria-label", "Phóng to đồ thị");
      zoomBtn.innerHTML = ZOOM_ICON_SVG;
      zoomBtn.style.cssText =
        "position:absolute;top:6px;right:6px;width:26px;height:26px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
        "justify-content:center;padding:0;";
      zoomBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openPlotZoomModal(functionPlot, spec);
      });
      wrapper.appendChild(zoomBtn);

      // Tải PNG — function-plot vẽ bằng SVG nên tận dụng thẳng
      // `exportSvgAsPng` (dùng chung với mermaid.ts/svgFigure.ts).
      const svgEl = chartEl.querySelector("svg");
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
          exportSvgAsPng(svgEl as SVGSVGElement, "do-thi.png").catch((err) =>
            console.warn("[snip-ai] Xuất PNG đồ thị thất bại:", err),
          );
        });
        wrapper.appendChild(downloadBtn);
      }

      pre.replaceWith(wrapper);
    } catch (e) {
      // Công thức AI sinh ra có thể sai cú pháp math-eval (hallucinate) —
      // KHÔNG để lỗi này làm vỡ cả câu trả lời, giữ nguyên khối code gốc.
      console.warn("[snip-ai] Không vẽ được đồ thị, giữ nguyên code gốc:", e);
      pre.dataset.plotTried = "1";
    }
  }
}

/** Action gắn vào container `.markdown-body` (result/+page.svelte và
 * history/+page.svelte) — cùng cách dùng với `mermaidBlocks`. */
export const plotBlocks: Action<HTMLElement, unknown> = (node) => {
  renderPlotBlocks(node);
  return {
    update() {
      renderPlotBlocks(node);
    },
  };
};

// ── Phóng to đồ thị (modal riêng) ───────────────────────────────────────────
// KHÁC hẳn cách zoom của Mermaid (openMermaidZoomModal trong mermaid.ts, tự
// viết pan/zoom bằng tay trên 1 chuỗi SVG tĩnh) — function-plot đã tự có sẵn
// pan/zoom (kéo thả + cuộn chuột, dựng trên d3-zoom) ngay khi khởi tạo, không
// cần viết lại. Ở đây chỉ cần dựng 1 modal to hơn rồi khởi tạo LẠI 1 instance
// function-plot MỚI, to hơn, từ ĐÚNG `spec` đã parse — không tái dùng SVG cũ
// (tái dùng sẽ mất mọi hành vi zoom/pan gắn kèm, xem cảnh báo trong JSDoc của
// hàm `functionPlot` upstream: options nên được tạo mới mỗi lần build chart
// mới, không phải "nhân bản" DOM đã vẽ sẵn).

const ZOOM_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/></svg>';

const DOWNLOAD_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg>';

function openPlotZoomModal(functionPlot: Awaited<ReturnType<typeof loadFunctionPlot>>, spec: PlotSpec): void {
  const overlay = document.createElement("div");
  overlay.style.cssText =
    "position:fixed;inset:0;z-index:9999;background:rgba(0,0,0,.82);" +
    "display:flex;align-items:center;justify-content:center;overflow:hidden;padding:40px;";

  const card = document.createElement("div");
  // Nền trắng cố định — cùng lý do với bản nhúng trong bong bóng chat ở
  // `renderPlotBlocks` (xem giải thích ở đó).
  card.style.cssText =
    "background:#fff;border-radius:14px;padding:16px;max-width:100%;max-height:100%;" +
    "box-shadow:0 20px 60px rgba(0,0,0,.4);";
  overlay.appendChild(card);

  const chartEl = document.createElement("div");
  card.appendChild(chartEl);

  const closeBtn = document.createElement("button");
  closeBtn.type = "button";
  closeBtn.title = "Đóng (Esc)";
  closeBtn.setAttribute("aria-label", "Đóng");
  closeBtn.textContent = "×";
  closeBtn.style.cssText =
    "position:absolute;top:16px;right:16px;width:34px;height:34px;border-radius:8px;" +
    "border:1px solid rgba(255,255,255,.25);background:rgba(255,255,255,.1);color:#fff;" +
    "font-size:20px;line-height:1;cursor:pointer;display:flex;align-items:center;justify-content:center;";
  overlay.appendChild(closeBtn);

  function close() {
    document.removeEventListener("keydown", onKeydown);
    overlay.remove();
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
  closeBtn.addEventListener("click", close);
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
  document.addEventListener("keydown", onKeydown);

  document.body.appendChild(overlay);

  // Kích thước lớn hơn hẳn bản nhúng trong bong bóng chat, nhưng vẫn chừa lề
  // để không tràn màn hình trên máy nhỏ.
  const width = Math.min(720, window.innerWidth - 120);
  const height = Math.min(520, window.innerHeight - 160);
  functionPlot(buildOptions(spec, chartEl, width, height));
}
