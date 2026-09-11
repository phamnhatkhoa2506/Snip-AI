<script lang="ts">
  // Ảnh + khung khoanh vùng AI chỉ tới (nếu có), dùng chung cho cả ảnh nhỏ
  // ngay trong bong bóng chat lẫn ảnh phóng to trong modal — tránh lặp lại
  // phần tính toán toạ độ ở 2 nơi.
  //
  // Điểm khó: ảnh hiển thị bằng `object-contain` bên trong khung có kích
  // thước riêng, nên vùng ẢNH THẬT nhìn thấy được có thể NHỎ HƠN khung chứa
  // (viền trống 2 bên nếu tỉ lệ khung # tỉ lệ ảnh). Toạ độ % của box_2d phải
  // tính theo vùng ảnh thật đó, không phải theo % của khung chứa.
  import { fade } from "svelte/transition";
  import Icon from "./Icon.svelte";

  type Box2d = [number, number, number, number]; // [ymin, xmin, ymax, xmax], thang 0-1000

  interface Props {
    src: string;
    /** null = không có khung nào để vẽ (câu trả lời không nhắc vị trí cụ thể). */
    box: Box2d | null;
    alt?: string;
    /** Class áp cho chính thẻ <img> — kích thước/viền/bo góc tự quyết định
     * theo ngữ cảnh dùng (nhỏ trong bong bóng chat, hay full trong modal). */
    class?: string;
    /** Bật kéo chỉnh khung + nút "Hỏi thêm" — CHỈ bật ở nơi đủ chỗ và cần độ
     * chính xác (ảnh phóng to). KHÔNG bật ở ảnh nhỏ trong bong bóng chat —
     * nhồi tay cầm + nút bấm vào 1 khung ~150px cao chỉ làm rối mắt, đúng
     * điều cần tránh ("gọn gọn"). */
    interactive?: boolean;
    /** Gọi khi bấm "Hỏi thêm", kèm khung HIỆN TẠI (đã kéo chỉnh nếu có).
     * Chỉ cần truyền khi `interactive` bật. */
    onAskRegion?: (box: Box2d) => void;
  }
  let { src, box, alt = "", class: imgClass = "", interactive = false, onAskRegion }: Props = $props();

  let imgEl = $state<HTMLImageElement | null>(null);
  let rect = $state<{ left: number; top: number; width: number; height: number } | null>(null);
  /** Hình học render hiện tại (khác kích thước ẢNH GỐC) — cần lưu lại để quy
   * đổi NGƯỢC từ pixel chuột về thang 0-1000 lúc kéo chỉnh. */
  let geom = $state<{ renderW: number; renderH: number; offsetX: number; offsetY: number } | null>(null);

  /** Khung ĐANG DÙNG để vẽ — khởi tạo theo `box` (AI trả về), nhưng tách
   * riêng để người dùng kéo chỉnh được mà KHÔNG ghi đè ngược lên prop gốc
   * (dữ liệu AI trả về chỉ nên đọc, không nên bị component con sửa trực tiếp). */
  let currentBox = $state<Box2d | null>(null);
  $effect(() => {
    // Chạy cả lúc mount (khởi tạo theo `box` ban đầu) lẫn mỗi khi `box` đổi
    // (AI trả lời khác) -> reset lại theo AI, bỏ chỉnh tay cũ.
    currentBox = box;
  });

  function recompute() {
    const el = imgEl;
    if (!el || !currentBox || !el.naturalWidth || !el.naturalHeight || !el.clientWidth || !el.clientHeight) {
      rect = null;
      geom = null;
      return;
    }
    const boxRatio = el.clientWidth / el.clientHeight;
    const imgRatio = el.naturalWidth / el.naturalHeight;
    let renderW: number, renderH: number, offsetX: number, offsetY: number;
    if (imgRatio > boxRatio) {
      // Ảnh "dẹt" hơn khung chứa -> lấp đầy theo chiều NGANG, chừa khoảng trống trên/dưới.
      renderW = el.clientWidth;
      renderH = el.clientWidth / imgRatio;
      offsetX = 0;
      offsetY = (el.clientHeight - renderH) / 2;
    } else {
      // Ảnh "cao" hơn khung chứa -> lấp đầy theo chiều DỌC, chừa khoảng trống trái/phải.
      renderH = el.clientHeight;
      renderW = el.clientHeight * imgRatio;
      offsetY = 0;
      offsetX = (el.clientWidth - renderW) / 2;
    }
    geom = { renderW, renderH, offsetX, offsetY };
    const [ymin, xmin, ymax, xmax] = currentBox;
    rect = {
      left: offsetX + (xmin / 1000) * renderW,
      top: offsetY + (ymin / 1000) * renderH,
      width: ((xmax - xmin) / 1000) * renderW,
      height: ((ymax - ymin) / 1000) * renderH,
    };
  }

  // Theo dõi cả kích thước PHẦN TỬ (đổi khi kéo giãn cửa sổ) lẫn `currentBox`
  // (đổi khi có câu trả lời mới, hoặc lúc kéo chỉnh) — thiếu 1 trong 2 thì
  // khung có thể lệch/không cập nhật đúng lúc.
  $effect(() => {
    const el = imgEl;
    if (!el) {
      rect = null;
      geom = null;
      return;
    }
    recompute();
    const ro = new ResizeObserver(recompute);
    ro.observe(el);
    return () => ro.disconnect();
  });

  // ── Kéo chỉnh khung (chỉ khi interactive) ──────────────────────────────
  // Mỗi tay cầm là 1 góc [fx,fy] (0|1, 0|1) — kéo góc nào thì góc ĐỐI DIỆN
  // giữ nguyên, chỉ đổi cạnh mà góc đang kéo phụ trách.
  let draggingCorner = $state<[number, number] | null>(null);
  /** Khoảng cách tối thiểu giữ giữa 2 cạnh đối diện (đơn vị thang 0-1000) —
   * ~2% kích thước ảnh, tránh kéo khung co về 0 mất luôn hình dạng. */
  const MIN_GAP = 20;

  function onHandlePointerDown(e: PointerEvent, fx: number, fy: number) {
    draggingCorner = [fx, fy];
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.stopPropagation();
  }

  function onHandlePointerMove(e: PointerEvent) {
    if (!draggingCorner || !geom || !imgEl || !currentBox) return;
    // BẮT BUỘC làm tròn về số nguyên — Rust nhận `region` dưới dạng
    // `[u32; 4]` (đúng quy ước box_2d gốc của Gemini, luôn là số nguyên).
    // Toạ độ AI trả về sẵn là số nguyên nên không lộ ra, nhưng toạ độ tính
    // từ vị trí chuột (pixel / renderW * 1000) luôn ra số thực — thiếu bước
    // làm tròn này thì bấm "Hỏi thêm" sau khi kéo chỉnh sẽ lỗi ngay
    // ("invalid type: floating point ..., expected u32") — lỗi thực tế đã gặp.
    const imgRect = imgEl.getBoundingClientRect();
    const bx = Math.round(Math.min(1000, Math.max(0, ((e.clientX - imgRect.left - geom.offsetX) / geom.renderW) * 1000)));
    const by = Math.round(Math.min(1000, Math.max(0, ((e.clientY - imgRect.top - geom.offsetY) / geom.renderH) * 1000)));

    const [fx, fy] = draggingCorner;
    let [ymin, xmin, ymax, xmax] = currentBox;
    if (fx === 0) xmin = Math.min(bx, xmax - MIN_GAP);
    else xmax = Math.max(bx, xmin + MIN_GAP);
    if (fy === 0) ymin = Math.min(by, ymax - MIN_GAP);
    else ymax = Math.max(by, ymin + MIN_GAP);

    currentBox = [ymin, xmin, ymax, xmax];
  }

  function onHandlePointerUp(e: PointerEvent) {
    draggingCorner = null;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  }
</script>

<!-- KHÔNG tự đặt max-w/max-h (theo %) ở khối bọc này — với nhiều lớp <div>
lồng nhau như ở modal xem ảnh, % chỉ tính đúng khi TOÀN BỘ chuỗi cha đều có
kích thước tường minh, chỉ 1 lớp bị "auto" là toàn chuỗi vỡ theo (bug thực tế
đã gặp: ảnh hiển thị to hơn khung, bị cắt). Để `class` truyền vào tự quyết
định giới hạn kích thước của <img> theo đơn vị KHÔNG phụ thuộc cha (vw/vh
hoặc px cố định) — xem cách gọi ở result/+page.svelte. -->
<div class="relative inline-block">
  <img bind:this={imgEl} onload={recompute} {src} {alt} class="{imgClass} block" />
  {#if rect}
    <!-- Cùng kiểu dáng với khung chọn vùng lúc snip (viền accent + 4 chấm
    góc + glow) — người dùng nhận ra ngay đây là "AI đang chỉ vào đâu đó",
    nhất quán trong toàn app. -->
    <div
      class="absolute border-2 border-accent rounded-sm {draggingCorner ? '' : 'pointer-events-none'}"
      style="left:{rect.left}px; top:{rect.top}px; width:{rect.width}px; height:{rect.height}px;
        box-shadow: 0 0 0 1px rgba(0,0,0,0.6), 0 0 16px -4px color-mix(in srgb, var(--color-accent) 55%, transparent);"
      transition:fade={{ duration: 180 }}
    >
      {#each [[0, 0], [1, 0], [0, 1], [1, 1]] as [fx, fy], ci (ci)}
        {#if interactive}
          <!-- Vùng chạm to hơn hẳn chấm hiển thị (chấm chỉ 6px, khó trỏ
          trúng để kéo) — 20px vô hình bao quanh, chấm màu vẫn nhỏ như cũ
          cho gọn mắt. -->
          <button
            type="button"
            aria-label="Kéo để chỉnh khung"
            class="absolute w-5 h-5 rounded-full pointer-events-auto cursor-grab touch-none bg-transparent border-none p-0"
            style="left:{fx * rect.width - 10}px; top:{fy * rect.height - 10}px;"
            onpointerdown={(e) => onHandlePointerDown(e, fx, fy)}
            onpointermove={onHandlePointerMove}
            onpointerup={onHandlePointerUp}
            onpointercancel={onHandlePointerUp}
          >
            <span
              class="absolute inset-0 m-auto w-1.5 h-1.5 rounded-full bg-accent"
              style="box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 85%, transparent);"
            ></span>
          </button>
        {:else}
          <div
            class="absolute w-1.5 h-1.5 rounded-full bg-accent"
            style="left:{fx * rect.width - 3}px; top:{fy * rect.height - 3}px;
              box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 85%, transparent);"
          ></div>
        {/if}
      {/each}

      {#if interactive && onAskRegion}
        <!-- 1 nút duy nhất, gọn (icon + 2 chữ) — neo NGAY vào góc dưới-phải
        khung, không chiếm thêm hàng/khoảng trống riêng trong layout. -->
        <button
          type="button"
          onclick={() => currentBox && onAskRegion?.(currentBox)}
          class="btn-accent absolute pointer-events-auto px-2.5 py-1 rounded-full text-[11px] font-semibold flex items-center gap-1 shadow-lg whitespace-nowrap"
          style="left:{rect.width}px; top:{rect.height}px; transform: translate(-100%, 6px);"
        >
          <Icon name="sparkles" size={10} strokeWidth={2.4} />
          Hỏi thêm
        </button>
      {/if}
    </div>
  {/if}
</div>
