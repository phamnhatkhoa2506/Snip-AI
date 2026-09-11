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

  type Box2d = [number, number, number, number]; // [ymin, xmin, ymax, xmax], thang 0-1000

  interface Props {
    src: string;
    /** null = không có khung nào để vẽ (câu trả lời không nhắc vị trí cụ thể). */
    box: Box2d | null;
    alt?: string;
    /** Class áp cho chính thẻ <img> — kích thước/viền/bo góc tự quyết định
     * theo ngữ cảnh dùng (nhỏ trong bong bóng chat, hay full trong modal). */
    class?: string;
  }
  let { src, box, alt = "", class: imgClass = "" }: Props = $props();

  let imgEl = $state<HTMLImageElement | null>(null);
  let rect = $state<{ left: number; top: number; width: number; height: number } | null>(null);

  function recompute() {
    const el = imgEl;
    if (!el || !box || !el.naturalWidth || !el.naturalHeight || !el.clientWidth || !el.clientHeight) {
      rect = null;
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
    const [ymin, xmin, ymax, xmax] = box;
    rect = {
      left: offsetX + (xmin / 1000) * renderW,
      top: offsetY + (ymin / 1000) * renderH,
      width: ((xmax - xmin) / 1000) * renderW,
      height: ((ymax - ymin) / 1000) * renderH,
    };
  }

  // Theo dõi cả kích thước PHẦN TỬ (đổi khi kéo giãn cửa sổ/bong bóng chat co
  // giãn) lẫn `box` (đổi khi component tái dùng cho 1 tin nhắn khác) — thiếu
  // 1 trong 2 thì khung có thể lệch/không cập nhật đúng lúc.
  $effect(() => {
    const el = imgEl;
    if (!el) {
      rect = null;
      return;
    }
    recompute();
    const ro = new ResizeObserver(recompute);
    ro.observe(el);
    return () => ro.disconnect();
  });
</script>

<div class="relative inline-block max-w-full max-h-full">
  <img bind:this={imgEl} onload={recompute} {src} {alt} class="{imgClass} block" />
  {#if rect}
    <!-- Cùng kiểu dáng với khung chọn vùng lúc snip (viền accent + 4 chấm
    góc + glow) — người dùng nhận ra ngay đây là "AI đang chỉ vào đâu đó",
    nhất quán trong toàn app. -->
    <div
      class="absolute border-2 border-accent pointer-events-none rounded-sm"
      style="left:{rect.left}px; top:{rect.top}px; width:{rect.width}px; height:{rect.height}px;
        box-shadow: 0 0 0 1px rgba(0,0,0,0.6), 0 0 16px -4px color-mix(in srgb, var(--color-accent) 55%, transparent);"
      transition:fade={{ duration: 180 }}
    >
      {#each [[0, 0], [1, 0], [0, 1], [1, 1]] as [fx, fy], ci (ci)}
        <div
          class="absolute w-1.5 h-1.5 rounded-full bg-accent"
          style="left:{fx * rect.width - 3}px; top:{fy * rect.height - 3}px;
            box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 85%, transparent);"
        ></div>
      {/each}
    </div>
  {/if}
</div>
