// Vẽ BIỂU ĐỒ SỐ LIỆU (cột, đường, tròn, radar, scatter, gauge, mạng lưới,
// treemap, sankey...) khi AI trả lời bằng khối mã ```chart — khác hẳn 3 khối
// đã có: `mermaid` (sơ đồ LOGIC/luồng, không phải số liệu), `plot` (đồ thị 1
// CÔNG THỨC toán, tính chính xác từ hàm số — xem plot.ts), `svg` (hình học
// AI tự vẽ tay — xem svgFigure.ts). Khối này dành cho khi câu hỏi cần SO
// SÁNH/TỔNG HỢP SỐ LIỆU THẬT (doanh thu theo tháng, kết quả khảo sát, điểm
// số...) — xem rule liên quan trong SYSTEM_PROMPT (ai.rs).
//
// Dùng Apache ECharts — KHÁC cách làm ở plot.ts (tự thiết kế 1 schema JSON
// RIÊNG, đơn giản hoá, vì API gốc của function-plot không phổ biến/model ít
// gặp lúc train). Ở ĐÂY thì NGƯỢC LẠI: cấu trúc `option` gốc của ECharts
// (title/xAxis/yAxis/series...) đã LÀ 1 khai báo JSON thuần, rất phổ biến
// trong dữ liệu train của mọi model AI (vô số ví dụ/tài liệu dùng đúng cú
// pháp này) — tự thiết kế thêm 1 lớp bọc riêng chỉ khiến model PHẢI HỌC LẠI 1
// quy ước mới, dễ bịa sai hơn là dùng thẳng cú pháp nó đã quen. Nên khối
// ```chart nhận THẲNG object `option` gốc của ECharts, không qua lớp dịch
// nào — an toàn vì đây thuần là DỮ LIỆU (JSON), không có cách nào nhúng mã
// thực thi được qua đường này (khác hẳn rủi ro của khối ```svg).
//
// Cùng pattern với mermaid.ts/plot.ts: markdown.ts để khối ```chart hiện
// thành code block bình thường trước, action `chartBlocks` bên dưới quét DOM
// SAU KHI đã chèn vào trang, thay bằng biểu đồ thật. Nặng nên dynamic import
// + chỉ đăng ký (echarts.use) đúng tập chart-type/component cần dùng (tree-
// shaking) thay vì cả thư viện, không tải nếu câu trả lời không có ```chart.
import type { Action } from "svelte/action";
import type { EChartsCoreOption, ECharts as EChartsInstance } from "echarts/core";
import { exportDataUrlAsPng } from "./exportImage";

let echartsPromise: ReturnType<typeof loadEcharts> | null = null;

async function loadEcharts() {
  const [core, charts, components, renderers] = await Promise.all([
    import("echarts/core"),
    import("echarts/charts"),
    import("echarts/components"),
    import("echarts/renderers"),
  ]);
  core.use([
    // Các loại biểu đồ đăng ký sẵn — phủ hầu hết nhu cầu thống kê thường gặp
    // (cột/đường/tròn/radar/scatter) LẪN vài loại nâng cao cho tương lai xa
    // hơn (gauge, funnel, heatmap, graph mạng lưới, treemap, sankey) — đã
    // CHỦ ĐỘNG chọn ECharts thay vì Chart.js chính vì lý do phủ rộng này.
    charts.BarChart,
    charts.LineChart,
    charts.PieChart,
    charts.RadarChart,
    charts.ScatterChart,
    charts.GaugeChart,
    charts.FunnelChart,
    charts.HeatmapChart,
    charts.GraphChart,
    charts.TreemapChart,
    charts.SankeyChart,
    components.TitleComponent,
    components.TooltipComponent,
    components.LegendComponent,
    components.GridComponent,
    components.DataZoomComponent,
    components.VisualMapComponent,
    components.RadarComponent,
    renderers.CanvasRenderer,
  ]);
  return core;
}

function ensureEcharts() {
  if (!echartsPromise) echartsPromise = loadEcharts();
  return echartsPromise;
}

function parseChartOption(source: string): EChartsCoreOption | null {
  try {
    const option = JSON.parse(source);
    // Kiểm tra tối thiểu — phải có `series` (mảng), thứ MỌI biểu đồ ECharts
    // đều cần, để loại sớm JSON hợp lệ nhưng rõ ràng không phải option biểu
    // đồ (VD lỡ AI dùng nhầm khối ```chart cho 1 JSON bất kỳ khác).
    if (!option || typeof option !== "object" || Array.isArray(option) || !Array.isArray(option.series)) {
      return null;
    }
    return option as EChartsCoreOption;
  } catch {
    return null;
  }
}

async function renderChartBlocks(container: HTMLElement): Promise<void> {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("code.language-chart"));
  if (codeEls.length === 0) return;

  const echarts = await ensureEcharts();

  for (const codeEl of codeEls) {
    const pre = codeEl.closest("pre");
    // `data-chart-tried`: cùng cơ chế `data-mermaid-tried`/`data-plot-tried`.
    if (!pre || pre.dataset.chartTried === "1") continue;
    pre.dataset.chartTried = "1";

    const source = codeEl.textContent ?? "";
    const option = parseChartOption(source);
    // JSON hỏng/thiếu `series` (AI hallucinate) — giữ nguyên khối code gốc.
    if (!option) continue;

    try {
      const wrapper = document.createElement("div");
      wrapper.className = "echart-chart";
      // Nền TRẮNG CỐ ĐỊNH — cùng lý do đã áp dụng cho mermaid/plot/svg (xem
      // giải thích ở các file đó): biểu đồ luôn dễ nhìn hơn trên nền trắng,
      // không đổi theo theme sáng/tối của app.
      wrapper.style.cssText =
        "position:relative;border-radius:10px;overflow:hidden;background:#fff;" +
        "border:1px solid #e2e2e2;display:inline-block;max-width:100%;";

      const chartEl = document.createElement("div");
      const width = Math.max(240, Math.min(420, container.clientWidth || 420));
      chartEl.style.cssText = `width:${width}px;height:260px;max-width:100%;`;
      wrapper.appendChild(chartEl);

      const instance = echarts.init(chartEl, undefined, { renderer: "canvas", width, height: 260 });
      instance.setOption(option);

      const zoomBtn = document.createElement("button");
      zoomBtn.type = "button";
      zoomBtn.title = "Phóng to biểu đồ";
      zoomBtn.setAttribute("aria-label", "Phóng to biểu đồ");
      zoomBtn.innerHTML = ZOOM_ICON_SVG;
      zoomBtn.style.cssText =
        "position:absolute;top:6px;right:6px;width:26px;height:26px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;display:flex;align-items:center;" +
        "justify-content:center;padding:0;";
      zoomBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        openChartZoomModal(echarts, option);
      });
      wrapper.appendChild(zoomBtn);

      // Tải PNG — ECharts tự có sẵn `getDataURL()` (đã vẽ canvas nội bộ, trả
      // thẳng base64 PNG), không cần tự dựng canvas trung gian như SVG.
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
        const dataUrl = instance.getDataURL({ type: "png", backgroundColor: "#fff", pixelRatio: 2 });
        exportDataUrlAsPng(dataUrl, "bieu-do.png").catch((err) =>
          console.warn("[snip-ai] Xuất PNG biểu đồ thất bại:", err),
        );
      });
      wrapper.appendChild(downloadBtn);

      pre.replaceWith(wrapper);
    } catch (e) {
      // Option AI sinh ra có thể sai field/giá trị (hallucinate) — KHÔNG để
      // lỗi này làm vỡ cả câu trả lời, giữ nguyên khối code gốc.
      console.warn("[snip-ai] Không vẽ được biểu đồ, giữ nguyên code gốc:", e);
    }
  }
}

/** Action gắn vào container `.markdown-body` — cùng cách dùng với
 * `mermaidBlocks`/`plotBlocks`/`svgFigureBlocks`. */
export const chartBlocks: Action<HTMLElement, unknown> = (node) => {
  renderChartBlocks(node);
  return {
    update() {
      renderChartBlocks(node);
    },
  };
};

// ── Phóng to biểu đồ (modal riêng) ──────────────────────────────────────────
// Cùng ý tưởng với `openPlotZoomModal` (plot.ts): ECharts vẽ bằng Canvas
// (khác SVG của Mermaid/function-plot), không thể "phóng to" 1 ảnh canvas đã
// vẽ sẵn mà không bị vỡ nét — phải khởi tạo LẠI 1 instance ECharts MỚI, to
// hơn, từ ĐÚNG `option` gốc, để tự vẽ lại nét ở đúng độ phân giải mới. ECharts
// tự có sẵn tương tác phóng to/kéo (dataZoom, tooltip theo con trỏ...) ngay
// trong instance mới này, không cần tự viết pan/zoom bằng tay.

const ZOOM_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/></svg>';

const DOWNLOAD_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg>';

function openChartZoomModal(echarts: Awaited<ReturnType<typeof loadEcharts>>, option: EChartsCoreOption): void {
  const overlay = document.createElement("div");
  overlay.style.cssText =
    "position:fixed;inset:0;z-index:9999;background:rgba(0,0,0,.82);" +
    "display:flex;align-items:center;justify-content:center;overflow:hidden;padding:40px;";

  const card = document.createElement("div");
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

  let instance: EChartsInstance | null = null;
  function close() {
    document.removeEventListener("keydown", onKeydown);
    instance?.dispose();
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

  const width = Math.min(760, window.innerWidth - 120);
  const height = Math.min(540, window.innerHeight - 160);
  chartEl.style.cssText = `width:${width}px;height:${height}px;`;
  instance = echarts.init(chartEl, undefined, { renderer: "canvas", width, height });
  instance.setOption(option);
}
