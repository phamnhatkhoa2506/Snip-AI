// Vẽ ĐỒ THỊ HÀM SỐ khi AI trả lời bằng khối mã ```plot — khác hẳn Mermaid
// (chỉ vẽ được sơ đồ logic/luồng, KHÔNG vẽ được đồ thị hàm số ĐÚNG TỈ LỆ) và
// khác khối ```svg (xem svgFigure.ts — hình học AI tự "vẽ tay" bằng SVG,
// không đảm bảo chuẩn số đo). Ở ĐÂY, AI CHỈ cần khai báo CÔNG THỨC + miền giá
// trị dạng JSON, việc TÍNH TOÁN VÀ VẼ CHÍNH XÁC (không đoán/hoạ toạ độ) giao
// hẳn cho `function-plot` (dựng trên d3) — xem rule mới trong SYSTEM_PROMPT
// (ai.rs) dặn AI dùng khối này cho đồ thị hàm số/khảo sát hàm.
//
// ANIMATION (chuyển động theo công thức): khai báo thêm `time` thì công thức
// được dùng biến thời gian `t` — đường cong biến đổi theo `t` (sóng lan
// truyền, đồ thị đổi tham số...) và/hoặc các điểm chuyển động có toạ độ
// x(t), y(t) (ném xiên, con lắc, dao động...), kèm nút Play/Pause + thanh kéo
// `t`. Vẫn giữ đúng nguyên tắc: AI CHỈ khai báo công thức, mọi vị trí ở mọi
// khung hình đều TÍNH TỪ CÔNG THỨC (bộ tính biểu thức của chính function-plot)
// — không có chuyện AI tự "hoạ" từng khung, và không chạy bất kỳ code JS nào
// do AI viết.
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
type FunctionPlotFn = ((options: FunctionPlotOptions) => { draw(): void }) & {
  /** Bộ tính biểu thức dựng sẵn của function-plot (cùng cú pháp với `fn`) —
   * dùng để tính toạ độ điểm chuyển động theo `t` (xem `movingPoints`), không
   * tự viết/nhúng thêm 1 bộ parser toán riêng. */
  $eval?: { builtIn: (meta: object, property: string, variables: object) => number };
};

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

/** Cú pháp JSON đơn giản AI phải theo (xem rule ```plot trong ai.rs) — CHỈ
 * khai báo công thức/miền giá trị/điểm đáng chú ý bằng SỐ THẬT, KHÔNG tự tính
 * toạ độ pixel hay tự vẽ path gì cả — function-plot lo hết phần vẽ chính xác
 * từ công thức, loại bỏ hẳn rủi ro AI "hoạ" sai tỉ lệ/điểm cực trị/giao điểm. */
interface PlotSpec {
  title?: string;
  xDomain?: [number, number];
  yDomain?: [number, number];
  functions?: { fn: string; color?: string; label?: string }[];
  points?: { x: number; y: number; label?: string }[];
  /** Có field này = bật animation: biến `t` chạy từ `from` tới `to`
   * (`duration` giây thật cho 1 lượt phát, `loop` phát lặp lại). */
  time?: { from?: number; to: number; duration?: number; loop?: boolean };
  /** Điểm chuyển động — toạ độ là BIỂU THỨC theo `t` (VD ném xiên:
   * x = "10*t", y = "10*t - 4.9*t^2"). `trail`: vẽ vệt quỹ đạo đã đi qua. */
  movingPoints?: { x: string; y: string; label?: string; color?: string; trail?: boolean }[];
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

interface TimeRange {
  from: number;
  to: number;
  /** Số giây thật cho 1 lượt phát từ `from` tới `to`. */
  duration: number;
  loop: boolean;
}

/** Chuẩn hoá `time` AI khai báo — thiếu/sai thì coi như KHÔNG có animation
 * (đồ thị tĩnh như cũ), không lỗi. `duration` mặc định = đúng khoảng `t` (nếu
 * `t` là giây vật lý thì phát ĐÚNG tốc độ thật), kẹp trong 2–12 giây để không
 * quá chớp nhoáng hay quá lê thê. */
function normalizeTime(spec: PlotSpec): TimeRange | null {
  const time = spec.time;
  if (!time || !Number.isFinite(time.to)) return null;
  const from = Number.isFinite(time.from) ? (time.from as number) : 0;
  if (!(time.to > from)) return null;
  const rawDuration = Number.isFinite(time.duration) ? (time.duration as number) : time.to - from;
  return { from, to: time.to, duration: Math.min(12, Math.max(2, rawDuration)), loop: time.loop !== false };
}

/** Dựng options cho function-plot từ `spec`, kèm hàm `update(t)` — gán lại
 * `t` vào đúng các datum cần đổi (scope của đường cong, vị trí điểm chuyển
 * động, vệt quỹ đạo, nhãn) rồi để caller gọi `chart.draw()`. Datum là object
 * GIỮ NGUYÊN qua các khung hình (chỉ sửa field), đúng cách function-plot
 * khuyến nghị để vẽ lại nhanh mà không dựng lại cả chart. */
function buildOptions(
  functionPlot: FunctionPlotFn,
  spec: PlotSpec,
  target: HTMLElement,
  width: number,
  height: number,
): { options: FunctionPlotOptions; update: (t: number) => void; time: TimeRange | null } {
  const time = normalizeTime(spec);
  // 1 object `scope` DÙNG CHUNG cho mọi đường cong — đổi `scope.t` 1 lần là
  // mọi công thức có `t` tự tính lại ở lần `draw()` kế tiếp.
  const scope = { t: time?.from ?? 0 };

  const data: FunctionPlotDatum[] = (spec.functions ?? [])
    .filter((f) => typeof f.fn === "string" && f.fn.trim())
    .map((f, i) => ({
      fn: f.fn,
      color: f.color || DEFAULT_COLORS[i % DEFAULT_COLORS.length],
      ...(time ? { scope } : {}),
    }));

  const points = (spec.points ?? []).filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
  if (points.length > 0) {
    data.push({
      points: points.map((p) => [p.x, p.y]),
      fnType: "points",
      graphType: "scatter",
      color: "#ef4444",
      skipTip: true,
      // Chấm mặc định của function-plot chỉ bán kính 1px, ruột gần như trắng
      // — gần như không thấy trên nền trắng.
      attr: { r: 4, fill: "#ef4444", opacity: 1 },
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

  // ── Điểm chuyển động (chỉ khi có `time`) ────────────────────────────────
  const evalBuiltIn = functionPlot.$eval?.builtIn;
  const movers = time && evalBuiltIn
    ? (spec.movingPoints ?? [])
        .filter((m) => typeof m.x === "string" && typeof m.y === "string")
        .map((m, i) => {
          const color = m.color || DEFAULT_COLORS[(i + 3) % DEFAULT_COLORS.length];
          // Object `meta` GIỮ NGUYÊN qua mọi khung hình — builtIn biên dịch
          // biểu thức 1 lần rồi cache ngay trên object này, không biên dịch
          // lại 30 lần/giây.
          const xMeta = { fn: m.x };
          const yMeta = { fn: m.y };
          const pointDatum = {
            points: [[0, 0]],
            fnType: "points",
            graphType: "scatter",
            color,
            skipTip: true,
            attr: { r: 6, fill: color, opacity: 1 },
          } as FunctionPlotDatum;
          const trailDatum =
            m.trail !== false
              ? ({ points: [[0, 0], [0, 0]], fnType: "points", graphType: "polyline", color, skipTip: true } as FunctionPlotDatum)
              : null;
          const labelDatum = m.label
            ? ({ graphType: "text", fnType: "points", location: [0, 0], text: m.label, skipTip: true } as FunctionPlotDatum)
            : null;
          return { xMeta, yMeta, pointDatum, trailDatum, labelDatum };
        })
    : [];
  for (const mv of movers) {
    if (mv.trailDatum) data.push(mv.trailDatum);
    data.push(mv.pointDatum);
    if (mv.labelDatum) data.push(mv.labelDatum);
  }

  const posAt = (mv: (typeof movers)[number], t: number): [number, number] => [
    evalBuiltIn!(mv.xMeta, "fn", { t }),
    evalBuiltIn!(mv.yMeta, "fn", { t }),
  ];

  function update(t: number) {
    scope.t = t;
    if (!time) return;
    for (const mv of movers) {
      const [x, y] = posAt(mv, t);
      if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
      mv.pointDatum.points = [[x, y]];
      if (mv.labelDatum) mv.labelDatum.location = [x, y];
      if (mv.trailDatum) {
        // Lấy mẫu quỹ đạo từ `from` tới `t` hiện tại — mật độ theo tỉ lệ
        // quãng đã đi (tối đa ~120 điểm cả lượt), polyline cần ≥ 2 điểm.
        const n = Math.max(2, Math.ceil(((t - time.from) / (time.to - time.from)) * 120) + 1);
        const trail: number[][] = [];
        for (let k = 0; k < n; k++) {
          const tk = time.from + ((t - time.from) * k) / (n - 1);
          const p = posAt(mv, tk);
          if (Number.isFinite(p[0]) && Number.isFinite(p[1])) trail.push(p);
        }
        mv.trailDatum.points = trail.length >= 2 ? trail : [[x, y], [x, y]];
      }
    }
  }

  // Tính SẴN khung đầu tiên trước khi dựng chart — không để chấm/vệt nằm ở
  // (0,0) giả 1 nhịp. Biểu thức sai cú pháp sẽ ném lỗi NGAY ở đây, rơi về
  // nhánh "giữ nguyên khối code gốc" của caller.
  update(scope.t);

  return {
    options: {
      target,
      width,
      height,
      title: spec.title,
      grid: true,
      xAxis: spec.xDomain ? { domain: spec.xDomain } : undefined,
      yAxis: spec.yDomain ? { domain: spec.yDomain } : undefined,
      data,
    },
    update,
    time,
  };
}

/** Dựng chart vào `chartEl` — nếu có `time` thì thêm thanh điều khiển
 * (Play/Pause + thanh kéo `t` + nhãn giá trị `t`) ngay dưới và TỰ PHÁT 1 lần.
 * Dùng chung cho bản nhúng trong bong bóng chat lẫn modal phóng to. */
function mountPlot(functionPlot: FunctionPlotFn, spec: PlotSpec, chartEl: HTMLElement, width: number, height: number): void {
  const { options, update, time } = buildOptions(functionPlot, spec, chartEl, width, height);
  const chart = functionPlot(options);
  if (!time) return;

  const bar = document.createElement("div");
  bar.style.cssText =
    "display:flex;align-items:center;gap:8px;padding:6px 10px 8px;border-top:1px solid #eee;" +
    "font:12px system-ui,sans-serif;color:#333;";
  const playBtn = document.createElement("button");
  playBtn.type = "button";
  playBtn.style.cssText =
    "width:28px;height:24px;border-radius:6px;border:1px solid #d4d4d4;background:#fafafa;color:#222;" +
    "cursor:pointer;display:flex;align-items:center;justify-content:center;padding:0;font-size:12px;";
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = String(time.from);
  slider.max = String(time.to);
  slider.step = String((time.to - time.from) / 500);
  slider.style.cssText = "flex:1;min-width:0;accent-color:#7c5cff;";
  const tLabel = document.createElement("span");
  tLabel.style.cssText = "min-width:62px;text-align:right;font-variant-numeric:tabular-nums;";
  bar.append(playBtn, slider, tLabel);
  chartEl.appendChild(bar);

  let t = time.from;
  let playing = false;
  let rafId = 0;
  let lastFrame = 0;
  let lastDraw = 0;

  function render() {
    try {
      update(t);
      chart.draw();
    } catch (e) {
      // Biểu thức lỗi ở 1 giá trị `t` nào đó (VD chia cho 0) — dừng phát,
      // giữ khung hình cuối cùng vẽ được, không làm vỡ giao diện.
      console.warn("[snip-ai] Lỗi tính khung hình animation, dừng phát:", e);
      pause();
    }
    slider.value = String(t);
    tLabel.textContent = `t = ${t.toFixed(2)}`;
  }

  function setPlaying(next: boolean) {
    playing = next;
    playBtn.textContent = playing ? "❚❚" : "▶";
    playBtn.title = playing ? "Tạm dừng" : "Phát";
    playBtn.setAttribute("aria-label", playBtn.title);
  }

  function pause() {
    setPlaying(false);
    cancelAnimationFrame(rafId);
  }

  function frame(now: number) {
    // Khối đồ thị đã bị gỡ khỏi trang (đóng modal, cửa sổ chat vẽ lại...) —
    // dừng hẳn vòng lặp, không chạy ngầm mãi.
    if (!chartEl.isConnected) {
      pause();
      return;
    }
    const dt = lastFrame ? (now - lastFrame) / 1000 : 0;
    lastFrame = now;
    t += (dt / time!.duration) * (time!.to - time!.from);
    if (t >= time!.to) {
      if (time!.loop) {
        t = time!.from;
      } else {
        t = time!.to;
        render();
        pause();
        return;
      }
    }
    // Vẽ lại tối đa ~30 khung/giây — đủ mượt cho minh hoạ, đỡ tốn CPU hơn
    // hẳn vẽ lại toàn bộ SVG ở 60 khung/giây.
    if (now - lastDraw >= 33) {
      lastDraw = now;
      render();
    }
    rafId = requestAnimationFrame(frame);
  }

  function play() {
    if (t >= time!.to) t = time!.from; // đã chạy hết (không lặp) -> phát lại từ đầu
    setPlaying(true);
    lastFrame = 0;
    rafId = requestAnimationFrame(frame);
  }

  playBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (playing) pause();
    else play();
  });
  // Kéo thanh `t` = tự xem từng khung — dừng phát tự động để khỏi bị giật
  // ngược lại vị trí cũ ngay khi thả tay.
  slider.addEventListener("input", () => {
    pause();
    t = Number(slider.value);
    render();
  });

  setPlaying(false);
  render();
  play();
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

    let wrapper: HTMLDivElement | null = null;
    try {
      wrapper = document.createElement("div");
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
      // Gắn vào trang TRƯỚC khi dựng — vòng lặp animation tự dừng khi
      // `chartEl` không còn trong trang (`isConnected`), dựng lúc chưa gắn
      // thì khung hình đầu tiên đã tưởng bị gỡ và dừng ngay.
      pre.replaceWith(wrapper);
      mountPlot(functionPlot, spec, chartEl, width, 260);

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
      // `exportSvgAsPng` (dùng chung với mermaid.ts/svgFigure.ts). Có
      // animation thì ra đúng khung hình ĐANG HIỆN lúc bấm.
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
    } catch (e) {
      // Công thức AI sinh ra có thể sai cú pháp math-eval (hallucinate) —
      // KHÔNG để lỗi này làm vỡ cả câu trả lời, giữ nguyên khối code gốc.
      console.warn("[snip-ai] Không vẽ được đồ thị, giữ nguyên code gốc:", e);
      // Wrapper đã được gắn vào trang TRƯỚC khi dựng (xem ghi chú ở trên) —
      // lỗi thì trả lại đúng khối code gốc vào chỗ cũ.
      if (wrapper?.isConnected) wrapper.replaceWith(pre);
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
// mới, không phải "nhân bản" DOM đã vẽ sẵn). Có animation thì modal cũng có
// thanh Play/Pause riêng (mountPlot dùng chung).

const ZOOM_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/></svg>';

const DOWNLOAD_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" ' +
  'stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg>';

function openPlotZoomModal(functionPlot: FunctionPlotFn, spec: PlotSpec): void {
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
    // Gỡ khỏi trang -> vòng lặp animation (nếu có) tự dừng ở khung kế tiếp
    // (xem kiểm tra `isConnected` trong mountPlot).
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
  const height = Math.min(520, window.innerHeight - 200);
  try {
    mountPlot(functionPlot, spec, chartEl, width, height);
  } catch (e) {
    console.warn("[snip-ai] Không dựng được đồ thị phóng to:", e);
  }
}
