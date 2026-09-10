<script lang="ts">
  // Thanh cuộn TỰ VẼ, không dùng scrollbar của trình duyệt.
  //
  // Vì sao phải tự vẽ thay vì style bằng CSS cho gọn: WebView2 (engine chạy
  // app này) từ runtime 119 trở đi PHỚT LỜ `::-webkit-scrollbar` — lỗi đã
  // được ghi nhận chính thức (MicrosoftEdge/WebView2Feedback#4131), scrollbar
  // bị ép về kiểu Fluent của hệ thống, style bao nhiêu cũng không ăn. Vì vậy
  // app ẩn hẳn scrollbar mặc định trong app.css và bất kỳ chỗ nào cần chỉ báo
  // cuộn thì bọc bằng component này.
  //
  // Cách hoạt động: vẫn để trình duyệt lo việc cuộn thật (`overflow-y: auto`,
  // giữ nguyên cuộn bằng chuột/trackpad/phím), chỉ ĐỌC `scrollTop`/
  // `scrollHeight` để vẽ 1 thanh chỉ báo phủ lên trên, và cho phép kéo thanh
  // đó để cuộn ngược lại.
  import type { Snippet } from "svelte";

  interface Props {
    /** Class cho khối bọc ngoài — nơi đặt kích thước/viền (VD "w-[280px] border-r"). */
    class?: string;
    /** Class cho khối NỘI DUNG bên trong — đặt padding ở đây chứ đừng đặt ở
     * `class`, để thanh cuộn nằm sát mép khung thay vì bị padding đẩy vào
     * trong (trông như thanh cuộn lơ lửng giữa khoảng trắng). */
    contentClass?: string;
    children: Snippet;
  }
  let { class: className = "", contentClass = "", children }: Props = $props();

  /** Thanh không bao giờ ngắn hơn mức này, kể cả khi nội dung rất dài — quá
   * ngắn thì khó trỏ chuột vào để kéo. */
  const MIN_THUMB_PX = 28;

  let viewport = $state<HTMLDivElement | null>(null);
  let content = $state<HTMLDivElement | null>(null);

  let thumbHeight = $state(0);
  let thumbTop = $state(0);
  /** false khi nội dung ngắn hơn khung -> ẩn hẳn thanh, không chiếm chỗ. */
  let scrollable = $state(false);
  let dragging = $state(false);

  function recompute() {
    const el = viewport;
    if (!el) return;
    const trackH = el.clientHeight;
    const maxScroll = el.scrollHeight - trackH;
    if (maxScroll <= 1) {
      scrollable = false;
      return;
    }
    scrollable = true;
    thumbHeight = Math.max(MIN_THUMB_PX, (trackH / el.scrollHeight) * trackH);
    thumbTop = (el.scrollTop / maxScroll) * (trackH - thumbHeight);
  }

  // Theo dõi CẢ khung nhìn LẪN nội dung bên trong: khung đổi khi người dùng
  // kéo giãn cửa sổ, nội dung đổi khi danh sách thêm/bớt mục hoặc ảnh vừa tải
  // xong (làm chiều cao nhảy). Thiếu 1 trong 2 là thanh cuộn sẽ sai tỉ lệ.
  $effect(() => {
    const el = viewport;
    const inner = content;
    if (!el || !inner) return;
    recompute();
    const ro = new ResizeObserver(recompute);
    ro.observe(el);
    ro.observe(inner);
    return () => ro.disconnect();
  });

  let dragStartY = 0;
  let dragStartScroll = 0;

  function onThumbPointerDown(e: PointerEvent) {
    if (!viewport) return;
    dragging = true;
    dragStartY = e.clientY;
    dragStartScroll = viewport.scrollTop;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    // Chặn hành vi bôi đen text khi kéo thanh cuộn.
    e.preventDefault();
  }

  function onThumbPointerMove(e: PointerEvent) {
    if (!dragging || !viewport) return;
    const trackH = viewport.clientHeight;
    const maxThumbTop = trackH - thumbHeight;
    if (maxThumbTop <= 0) return;
    // Quy đổi quãng kéo của THANH sang quãng cuộn của NỘI DUNG theo đúng tỉ lệ.
    const ratio = (e.clientY - dragStartY) / maxThumbTop;
    viewport.scrollTop = dragStartScroll + ratio * (viewport.scrollHeight - trackH);
  }

  function onThumbPointerUp(e: PointerEvent) {
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  }

  /** Bấm vào phần rãnh TRỐNG (không phải thanh) -> nhảy tới đúng chỗ đó. */
  function onTrackPointerDown(e: PointerEvent) {
    if (!viewport || e.target !== e.currentTarget) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const trackH = viewport.clientHeight;
    const maxThumbTop = trackH - thumbHeight;
    if (maxThumbTop <= 0) return;
    const wanted = Math.min(Math.max(e.clientY - rect.top - thumbHeight / 2, 0), maxThumbTop);
    viewport.scrollTop = (wanted / maxThumbTop) * (viewport.scrollHeight - trackH);
  }
</script>

<div class="cs-area relative {className}">
  <div bind:this={viewport} onscroll={recompute} class="h-full overflow-y-auto">
    <!-- `min-h-full flex flex-col` để nội dung con vẫn dùng được `flex-1` mà
    căn giữa theo chiều dọc (VD trạng thái rỗng/đang tải) — nếu để chiều cao
    tự do thì `h-full`/`flex-1` bên trong mất chỗ dựa và bị dồn lên đầu. -->
    <div bind:this={content} class="min-h-full flex flex-col {contentClass}">
      {@render children()}
    </div>
  </div>

  {#if scrollable}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="cs-track {dragging ? 'is-dragging' : ''}" onpointerdown={onTrackPointerDown}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="cs-thumb"
        style="top:{thumbTop}px; height:{thumbHeight}px;"
        onpointerdown={onThumbPointerDown}
        onpointermove={onThumbPointerMove}
        onpointerup={onThumbPointerUp}
        onpointercancel={onThumbPointerUp}
      ></div>
    </div>
  {/if}
</div>
