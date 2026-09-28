<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Icon from "$lib/Icon.svelte";
  import { allowedAudioSources, loadSettings, MIC_PERMISSION_ERROR_PREFIX, type AudioSnapSource } from "$lib/settings";

  // Thanh công cụ nổi của Snap Audio — cùng kiểu với thanh quay video
  // (record-toolbar), thêm dải sóng âm để thấy ngay micro/âm thanh máy có
  // đang thu được tiếng hay không. Tự đọc cài đặt nguồn + quyền rồi gọi
  // `start_audio_snap` (Rust không đọc được localStorage). Cửa sổ này KHÔNG
  // tự đóng — audio_snap.rs đóng nó sau khi đã xử lý xong đoạn ghi âm.
  const MAX_SECONDS = 120;
  const appendTo = new URLSearchParams(window.location.search).get("appendTo");

  type Phase = "starting" | "recording" | "blocked";
  let phase = $state<Phase>("starting");
  let message = $state("");
  /** Windows đang chặn quyền micro (khác "app chưa được cho phép" — cái đó
   * sửa trong Cài đặt của app). */
  let windowsBlocked = $state(false);
  let elapsedSec = $state(0);
  let busy = $state(false);
  let sourceLabel = $state("");

  const BAR_COUNT = 16;
  let bars = $state<number[]>(Array(BAR_COUNT).fill(0.08));

  const mm = $derived(String(Math.floor(elapsedSec / 60)).padStart(2, "0"));
  const ss = $derived(String(elapsedSec % 60).padStart(2, "0"));

  const SOURCE_LABELS: Record<AudioSnapSource, string> = {
    mic: "Micro",
    system: "Âm thanh máy",
    both: "Micro + âm thanh máy",
  };

  let timer: ReturnType<typeof setInterval> | undefined;

  async function start() {
    const s = loadSettings();
    const choice = s.audioSnapSource;
    const sources = allowedAudioSources(s, choice);
    if (sources.length === 0) {
      phase = "blocked";
      message =
        choice === "mic"
          ? "Chưa cho phép dùng micro"
          : choice === "system"
            ? "Chưa cho phép thu âm thanh máy"
            : "Chưa cho phép micro/âm thanh máy";
      return;
    }
    // Chọn "cả hai" nhưng mới cho phép 1 nguồn — vẫn ghi nguồn đã được phép,
    // hiện đúng tên nguồn thật đang thu để người dùng không hiểu nhầm.
    sourceLabel = sources.length === 2 ? SOURCE_LABELS.both : SOURCE_LABELS[sources[0]];
    try {
      await invoke("start_audio_snap", { sources, appendTo, autoCopy: s.autoCopyOnCapture });
      phase = "recording";
      const startedAt = Date.now();
      timer = setInterval(() => {
        elapsedSec = Math.min(MAX_SECONDS, Math.floor((Date.now() - startedAt) / 1000));
      }, 250);
    } catch (e) {
      const msg = String(e);
      windowsBlocked = msg.includes(MIC_PERMISSION_ERROR_PREFIX);
      message = windowsBlocked ? "Windows đang chặn micro" : msg;
      phase = "blocked";
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    listen<number>("audio-snap:level", (e) => {
      bars = [...bars.slice(1), Math.max(0.08, e.payload)];
    }).then((fn) => (unlisten = fn));
    start();
    return () => {
      clearInterval(timer);
      unlisten?.();
    };
  });

  async function handleStop() {
    busy = true;
    try {
      await invoke("stop_audio_snap");
    } catch (e) {
      message = String(e);
      busy = false;
    }
  }

  function handleCancel() {
    busy = true;
    invoke("cancel_audio_snap");
  }

  async function handleFixPermission() {
    if (windowsBlocked) {
      await invoke("open_mic_privacy_settings").catch(() => {});
    } else {
      await invoke("show_settings_window").catch(() => {});
    }
    invoke("cancel_audio_snap");
  }
</script>

<div class="w-screen h-screen flex items-center justify-center p-1.5">
  <div class="glass border border-border rounded-full h-full w-full flex items-center gap-2.5 px-3 shadow-lg">
    {#if phase === "blocked"}
      <Icon name="micOff" size={15} class="shrink-0 text-[color:var(--color-danger)]" />
      <span class="text-[11.5px] font-medium flex-1 min-w-0 truncate" title={message}>{message}</span>
      <button
        onclick={handleFixPermission}
        class="btn-accent shrink-0 px-2.5 py-1 rounded-full text-[11px] font-semibold"
      >
        {windowsBlocked ? "Mở quyền Windows" : "Mở Cài đặt"}
      </button>
      <button
        onclick={handleCancel}
        title="Đóng"
        class="btn-ghost w-7 h-7 rounded-full flex items-center justify-center shrink-0"
      >
        <Icon name="x" size={14} />
      </button>
    {:else}
      <span class="w-2 h-2 rounded-full pulse-ring shrink-0" style="background: var(--color-danger);"></span>
      <span class="text-[12.5px] font-mono font-semibold tabular-nums shrink-0">{mm}:{ss}</span>

      <div class="flex-1 min-w-0 h-6 flex items-center gap-[2px]" title={sourceLabel}>
        {#each bars as b, i (i)}
          <span
            class="flex-1 max-w-[4px] rounded-full transition-[height] duration-100"
            style="height: {Math.round(b * 100)}%; background: var(--color-accent);"
          ></span>
        {/each}
      </div>

      <button
        onclick={handleStop}
        disabled={busy || phase !== "recording"}
        title="Dừng ghi âm (gửi cho AI)"
        class="btn-ghost w-7 h-7 rounded-full flex items-center justify-center shrink-0 disabled:opacity-50"
      >
        <span class="w-2.5 h-2.5 rounded-[2px]" style="background: currentColor;"></span>
      </button>
      <button
        onclick={handleCancel}
        disabled={busy}
        title="Huỷ (không giữ đoạn ghi âm)"
        class="btn-ghost w-7 h-7 rounded-full flex items-center justify-center shrink-0 disabled:opacity-50 hover:!text-[color:var(--color-danger)]"
      >
        <Icon name="x" size={14} />
      </button>
    {/if}
  </div>
</div>
