<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";

  // Thanh công cụ nổi lúc đang quay video — kiểu Windows Snipping Tool (chấm
  // đỏ + đồng hồ đếm giờ + nút Dừng/Huỷ). Cửa sổ này KHÔNG tự đóng khi bấm nút
  // — backend (record.rs) mới là nơi thật sự đóng nó, sau khi encoder đã
  // flush xong file MP4 (bấm xong vẫn cần vài trăm ms xử lý), nên chỉ khoá nút
  // + hiện "Đang xử lý…" trong lúc chờ.
  const MAX_SECONDS = 30;
  let elapsedSec = $state(0);
  let busy = $state(false);
  let error = $state("");

  const mm = $derived(String(Math.floor(elapsedSec / 60)).padStart(2, "0"));
  const ss = $derived(String(elapsedSec % 60).padStart(2, "0"));

  onMount(() => {
    const startedAt = Date.now();
    const timer = setInterval(() => {
      elapsedSec = Math.min(MAX_SECONDS, Math.floor((Date.now() - startedAt) / 1000));
    }, 250);
    return () => clearInterval(timer);
  });

  async function handleStop() {
    busy = true;
    error = "";
    try {
      await invoke("stop_recording");
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  async function handleCancel() {
    busy = true;
    error = "";
    try {
      await invoke("cancel_recording");
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<div class="w-screen h-screen flex items-center justify-center p-1.5">
  <div class="glass border border-border rounded-full h-full w-full flex items-center gap-2.5 px-3 shadow-lg">
    <span class="w-2 h-2 rounded-full pulse-ring shrink-0" style="background: var(--color-danger);"></span>
    <span class="text-[12.5px] font-mono font-semibold tabular-nums shrink-0">{mm}:{ss}</span>
    <span class="text-[10px] text-text-muted shrink-0">/ 00:{MAX_SECONDS}</span>

    <div class="flex-1"></div>

    <button
      onclick={handleStop}
      disabled={busy}
      title="Dừng quay (giữ video)"
      class="btn-ghost w-7 h-7 rounded-full flex items-center justify-center shrink-0 disabled:opacity-50"
    >
      <span class="w-2.5 h-2.5 rounded-[2px]" style="background: currentColor;"></span>
    </button>
    <button
      onclick={handleCancel}
      disabled={busy}
      title="Huỷ quay (không giữ video)"
      class="btn-ghost w-7 h-7 rounded-full flex items-center justify-center shrink-0 disabled:opacity-50 hover:!text-[color:var(--color-danger)]"
    >
      <Icon name="x" size={14} />
    </button>
  </div>

  {#if error}
    <div
      class="absolute top-full mt-2 left-1/2 -translate-x-1/2 text-[11px] text-[color:var(--color-danger)] glass border border-border px-3 py-2 rounded-lg whitespace-nowrap"
    >
      {error}
    </div>
  {/if}
</div>
