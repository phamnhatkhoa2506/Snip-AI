<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  let imgSrc = $state("");
  let selecting = $state(false);
  let hasSelection = $state(false);
  let startX = $state(0);
  let startY = $state(0);
  let curX = $state(0);
  let curY = $state(0);
  let error = $state("");

  // Cửa sổ overlay DÙNG CHUNG cho cả snip ảnh lẫn quay video — nhận biết qua
  // query param `?mode=record` (xem commands.rs::trigger_recording). Nhờ
  // vậy màu khung chọn vùng/hiệu ứng giống HỆT nhau giữa 2 tính năng, không
  // phải tự tay đồng bộ màu ở 2 nơi khác nhau.
  const isRecordMode = new URLSearchParams(window.location.search).get("mode") === "record";

  // Rect đã chuẩn hoá (x,y luôn là góc trên-trái) theo đơn vị CSS/logical px.
  const rect = $derived({
    x: Math.min(startX, curX),
    y: Math.min(startY, curY),
    w: Math.abs(curX - startX),
    h: Math.abs(curY - startY),
  });

  const clipInset = $derived(
    `${rect.y}px ${window.innerWidth - (rect.x + rect.w)}px ${window.innerHeight - (rect.y + rect.h)}px ${rect.x}px`,
  );

  async function loadScreenshot() {
    error = "";
    try {
      const b64 = await invoke<string>("get_screenshot_base64");
      imgSrc = `data:image/png;base64,${b64}`;
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    // Cửa sổ overlay giờ luôn được TẠO MỚI mỗi lần chụp (không ẩn/hiện tái
    // dùng), nên onMount tự nhiên chạy fresh mỗi lần — không cần lắng nghe
    // event "reset" nữa.
    loadScreenshot();

    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        invoke("cancel_overlay");
      }
    };
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function onMouseDown(e: MouseEvent) {
    selecting = true;
    hasSelection = false;
    startX = curX = e.clientX;
    startY = curY = e.clientY;
  }

  function onMouseMove(e: MouseEvent) {
    if (!selecting) return;
    curX = e.clientX;
    curY = e.clientY;
    hasSelection = true;
  }

  async function onMouseUp() {
    if (!selecting) return;
    selecting = false;

    if (rect.w < 4 || rect.h < 4) {
      // Vùng chọn quá nhỏ -> coi như click nhầm, không làm gì (không huỷ overlay
      // để người dùng có thể kéo lại ngay).
      hasSelection = false;
      return;
    }

    const dpr = window.devicePixelRatio || 1;
    const physX = Math.round(rect.x * dpr);
    const physY = Math.round(rect.y * dpr);
    const physW = Math.round(rect.w * dpr);
    const physH = Math.round(rect.h * dpr);

    try {
      if (isRecordMode) {
        await invoke("start_region_recording", { x: physX, y: physY, width: physW, height: physH });
      } else {
        await invoke("crop_and_open_result", { x: physX, y: physY, width: physW, height: physH });
      }
    } catch (e) {
      error = String(e);
    }
  }
</script>

<svelte:window onmousemove={onMouseMove} onmouseup={onMouseUp} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="relative w-screen h-screen overflow-hidden cursor-crosshair select-none"
  role="application"
  onmousedown={onMouseDown}
>
  {#if imgSrc}
    <img src={imgSrc} alt="" class="absolute inset-0 w-full h-full block" style="filter: brightness(0.4)" draggable="false" />
    {#if hasSelection}
      <img
        src={imgSrc}
        alt=""
        class="absolute inset-0 w-full h-full block pointer-events-none"
        style="clip-path: inset({clipInset})"
        draggable="false"
      />
      <div
        class="absolute border-2 border-accent pointer-events-none"
        style="left:{rect.x}px; top:{rect.y}px; width:{rect.w}px; height:{rect.h}px; box-shadow: 0 0 0 1px rgba(0,0,0,0.6), 0 0 24px -4px color-mix(in srgb, var(--color-accent) 55%, transparent);"
      ></div>
      <!-- 4 chấm góc cho cảm giác "chọn vùng" rõ ràng hơn -->
      {#each [[0, 0], [1, 0], [0, 1], [1, 1]] as [fx, fy], ci (ci)}
        <div
          class="absolute w-2 h-2 rounded-full bg-accent pointer-events-none"
          style="left:{rect.x + fx * rect.w - 4}px; top:{rect.y + fy * rect.h - 4}px; box-shadow: 0 0 8px color-mix(in srgb, var(--color-accent) 85%, transparent);"
        ></div>
      {/each}
      <div
        class="absolute text-[11px] font-mono font-semibold text-accent-text px-2 py-0.5 rounded-md pointer-events-none"
        style="left:{rect.x}px; top:{Math.max(0, rect.y - 24)}px; background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
      >
        {rect.w} × {rect.h}
      </div>
    {/if}
  {/if}

  <div
    class="absolute top-7 left-1/2 -translate-x-1/2 text-[13px] text-white/90 px-4 py-2 rounded-full pointer-events-none glass border border-border flex items-center gap-2"
  >
    <span class="w-1.5 h-1.5 rounded-full bg-accent"></span>
    Kéo chuột để chọn vùng {isRecordMode ? "quay" : ""} ·
    <kbd class="px-1.5 py-0.5 rounded bg-white/10 text-[11px]">Esc</kbd> để huỷ
    <span class="w-px h-4 bg-white/20"></span>
    <!--
      Nút huỷ bấm được. Thanh gợi ý bọc ngoài để `pointer-events-none` (để
      không chắn thao tác kéo chọn vùng bên dưới nó), nên nút phải tự bật lại
      `pointer-events-auto`. `stopPropagation` ở mousedown là BẮT BUỘC: div
      phủ toàn màn hình phía dưới bắt mousedown để bắt đầu kéo vùng chọn, nếu
      không chặn thì cú bấm vào nút X vừa huỷ vừa khởi tạo một vùng chọn mới.
    -->
    <button
      type="button"
      aria-label="Huỷ"
      class="pointer-events-auto -mr-1.5 w-6 h-6 grid place-items-center rounded-full text-white/70 transition-colors hover:bg-white/15 hover:text-white cursor-pointer"
      onmousedown={(e) => e.stopPropagation()}
      onclick={() => invoke("cancel_overlay")}
    >
      <svg viewBox="0 0 24 24" class="w-3.5 h-3.5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
        <path d="M6 6l12 12M18 6L6 18" />
      </svg>
    </button>
  </div>

  {#if error}
    <div
      class="absolute bottom-7 left-1/2 -translate-x-1/2 text-[13px] text-[color:var(--color-danger)] glass border border-border px-4 py-2.5 rounded-xl"
    >
      {error}
    </div>
  {/if}
</div>
