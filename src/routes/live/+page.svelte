<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import Icon from "$lib/Icon.svelte";
  import ScrollArea from "$lib/ScrollArea.svelte";
  import { loadSettings, saveSettings, MIC_PERMISSION_ERROR_PREFIX } from "$lib/settings";

  // Trò chuyện trực tiếp bằng giọng nói (Gemini Live API) — toàn bộ âm thanh
  // (thu micro, phát giọng AI, gửi/nhận WebSocket) chạy ở Rust (live.rs);
  // trang này chỉ hiển thị trạng thái + lời thoại 2 phía và gửi lệnh
  // tắt mic/kết thúc.

  type Phase = "permission" | "connecting" | "listening" | "speaking" | "ended";
  let phase = $state<Phase>("connecting");
  let error = $state("");
  let notice = $state("");
  /** Windows đang chặn micro (khác "app chưa được cho phép"). */
  let windowsBlocked = $state(false);
  let permissionBusy = $state(false);

  let muted = $state(false);
  let headphones = $state(false);
  let level = $state(0);

  interface Line {
    role: "user" | "model";
    text: string;
    /** Đã chốt — lượt nói kế tiếp cùng vai sẽ mở dòng MỚI thay vì nối vào. */
    done: boolean;
    interrupted?: boolean;
  }
  let lines = $state<Line[]>([]);
  let transcriptEl = $state<HTMLDivElement | null>(null);

  const active = $derived(phase === "listening" || phase === "speaking");

  async function scrollToBottom() {
    await tick();
    transcriptEl?.scrollTo({ top: transcriptEl.scrollHeight, behavior: "smooth" });
  }

  /** Lời thoại đến từng mẩu nhỏ (vài từ 1 lần) — nối vào dòng đang mở của
   * đúng vai đó; đổi vai thì chốt dòng cũ. */
  function appendTranscript(role: "user" | "model", text: string) {
    const last = lines[lines.length - 1];
    if (last && last.role === role && !last.done) {
      last.text += text;
    } else {
      if (last && !last.done) last.done = true;
      lines.push({ role, text: text.trimStart(), done: false });
    }
    scrollToBottom();
  }

  function closeOpenLines() {
    for (const l of lines) l.done = true;
  }

  async function start() {
    error = "";
    notice = "";
    windowsBlocked = false;
    const s = loadSettings();
    headphones = s.liveHeadphones;
    if (!s.micAllowed) {
      phase = "permission";
      return;
    }
    phase = "connecting";
    muted = false;
    try {
      await invoke("live_start", { headphones });
      // live_start chỉ trả về khi phiên ĐÃ sẵn sàng — không phụ thuộc hoàn
      // toàn vào sự kiện "live:state" để rời khỏi trạng thái đang kết nối.
      if (phase === "connecting") phase = "listening";
    } catch (e) {
      const msg = String(e);
      windowsBlocked = msg.includes(MIC_PERMISSION_ERROR_PREFIX);
      error = msg.replace(MIC_PERMISSION_ERROR_PREFIX, "");
      phase = windowsBlocked ? "permission" : "ended";
    }
  }

  async function allowMic() {
    permissionBusy = true;
    error = "";
    windowsBlocked = false;
    try {
      await invoke("probe_microphone");
      saveSettings({ ...loadSettings(), micAllowed: true });
      await start();
    } catch (e) {
      const msg = String(e);
      windowsBlocked = msg.includes(MIC_PERMISSION_ERROR_PREFIX);
      error = msg.replace(MIC_PERMISSION_ERROR_PREFIX, "");
    } finally {
      permissionBusy = false;
    }
  }

  function toggleMute() {
    muted = !muted;
    invoke("live_set_muted", { muted });
  }

  function toggleHeadphones() {
    headphones = !headphones;
    saveSettings({ ...loadSettings(), liveHeadphones: headphones });
    invoke("live_set_headphones", { headphones });
  }

  function endCall() {
    invoke("live_stop");
  }

  onMount(() => {
    const unlistens: UnlistenFn[] = [];
    const on = <T,>(event: string, handler: (payload: T) => void) =>
      listen<T>(event, (e) => handler(e.payload)).then((fn) => unlistens.push(fn));

    // Đăng ký HẾT listener rồi mới bắt đầu — live_start bắn "live:state"
    // ngay trước khi trả về.
    Promise.all([
      on<string>("live:state", (s) => {
        if (s === "listening" || s === "speaking") phase = s;
      }),
      on<number>("live:level", (l) => (level = l)),
      on<{ role: "user" | "model"; text: string }>("live:transcript", (p) => appendTranscript(p.role, p.text)),
      on<null>("live:turn-complete", () => closeOpenLines()),
      on<null>("live:interrupted", () => {
        const last = lines.findLast((l) => l.role === "model" && !l.done);
        if (last) {
          last.done = true;
          last.interrupted = true;
        }
      }),
      on<string>("live:notice", (n) => (notice = n)),
      on<string>("live:ended", (reason) => {
        closeOpenLines();
        level = 0;
        phase = "ended";
        if (reason) notice = reason;
      }),
    ])
      .then(start)
      .catch((e) => {
        // VD cửa sổ chưa được cấp quyền nghe sự kiện (capabilities) — báo
        // lỗi thay vì đứng mãi ở "Đang kết nối…" không nói gì.
        error = `Không khởi tạo được cửa sổ trò chuyện: ${e}`;
        phase = "ended";
      });

    return () => {
      unlistens.forEach((fn) => fn());
      invoke("live_stop").catch(() => {});
    };
  });

  const statusText = $derived(
    phase === "connecting"
      ? "Đang kết nối…"
      : phase === "speaking"
        ? "AI đang nói…"
        : phase === "listening"
          ? muted
            ? "Micro đang tắt"
            : "Đang nghe — cứ nói tự nhiên"
          : phase === "ended"
            ? "Đã kết thúc"
            : "Cần quyền dùng micro",
  );

  /** Quả cầu to ra theo âm lượng micro (lúc nghe) hoặc "thở" đều (lúc AI nói). */
  const orbScale = $derived(phase === "listening" && !muted ? 1 + Math.min(level, 1) * 0.35 : 1);
</script>

<div class="app-bg h-screen flex flex-col text-text overflow-hidden">
  <div class="shrink-0 flex flex-col items-center gap-2.5 pt-6 pb-4 px-4">
    <div class="relative w-24 h-24 flex items-center justify-center">
      <div
        class="absolute inset-0 rounded-full transition-transform duration-100 {phase === 'speaking' ? 'pulse-ring' : ''}"
        style="transform: scale({orbScale}); background: radial-gradient(circle at 35% 30%, var(--color-accent-2), var(--color-accent)); opacity: {active
          ? 1
          : 0.45};"
      ></div>
      <span class="relative text-accent-text">
        {#if phase === "connecting"}
          <Icon name="loader" size={30} class="animate-spin" />
        {:else if phase === "speaking"}
          <Icon name="volume" size={30} />
        {:else if muted || phase === "permission"}
          <Icon name="micOff" size={30} />
        {:else}
          <Icon name="mic" size={30} />
        {/if}
      </span>
    </div>
    <div class="text-[13px] font-semibold">{statusText}</div>
    {#if notice}
      <div class="text-[11px] text-text-muted text-center max-w-[300px]" transition:fade={{ duration: 140 }}>{notice}</div>
    {/if}
  </div>

  {#if phase === "permission"}
    <div class="flex-1 flex flex-col items-center justify-center gap-3 px-6 text-center">
      <p class="text-[12.5px] text-text-muted leading-relaxed max-w-[300px]">
        Trò chuyện trực tiếp cần dùng micro. Windows không tự hỏi quyền cho app desktop, nên bạn cần cho phép tại đây.
      </p>
      {#if windowsBlocked}
        <button
          onclick={() => invoke("open_mic_privacy_settings")}
          class="btn-accent px-4 py-2 rounded-lg text-[12.5px] font-semibold"
        >
          Mở cài đặt quyền micro của Windows
        </button>
        <button onclick={allowMic} disabled={permissionBusy} class="btn-ghost px-3 py-1.5 rounded-lg text-[12px]">
          Thử lại
        </button>
      {:else}
        <button
          onclick={allowMic}
          disabled={permissionBusy}
          class="btn-accent px-4 py-2 rounded-lg text-[12.5px] font-semibold flex items-center gap-1.5 disabled:opacity-60"
        >
          {#if permissionBusy}<Icon name="loader" size={13} class="animate-spin" />{:else}<Icon name="mic" size={13} />{/if}
          Cho phép dùng micro
        </button>
      {/if}
      {#if error}
        <p class="text-[11.5px] text-[color:var(--color-danger)] leading-relaxed selectable">{error}</p>
      {/if}
    </div>
  {:else}
    <ScrollArea bind:viewport={transcriptEl} class="selectable flex-1 min-h-0" contentClass="px-3 py-2 flex flex-col gap-2.5">
      {#if lines.length === 0 && active}
        <p class="text-[11.5px] text-text-muted text-center mt-4">Lời thoại sẽ hiện ở đây.</p>
      {/if}
      {#each lines as line, i (i)}
        {#if line.role === "user"}
          <div class="flex justify-end msg-in">
            <div
              class="max-w-[85%] rounded-2xl rounded-br-md px-3 py-1.5 text-[12.5px] leading-relaxed text-accent-text"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              {line.text}
            </div>
          </div>
        {:else}
          <div class="flex msg-in">
            <div class="card max-w-[85%] rounded-2xl rounded-tl-md px-3 py-1.5 text-[12.5px] leading-relaxed">
              {line.text}{#if line.interrupted}<span class="text-text-muted"> — (bị ngắt)</span>{/if}
            </div>
          </div>
        {/if}
      {/each}
      {#if error}
        <div class="card px-3 py-2 text-[12px] text-[color:var(--color-danger)] flex items-start gap-2">
          <Icon name="alert" size={14} class="shrink-0 mt-0.5" />
          <span class="leading-relaxed">{error}</span>
        </div>
      {/if}
    </ScrollArea>

    <div class="shrink-0 p-3 flex items-center justify-center gap-2.5">
      {#if phase === "ended"}
        <button
          onclick={start}
          class="btn-accent px-5 py-2.5 rounded-full text-[13px] font-semibold flex items-center gap-2"
        >
          <Icon name="phone" size={15} /> Bắt đầu lại
        </button>
      {:else}
        <button
          onclick={toggleMute}
          disabled={!active}
          title={muted ? "Bật micro" : "Tắt micro"}
          class="w-11 h-11 rounded-full flex items-center justify-center transition-colors disabled:opacity-40 {muted
            ? 'btn-accent'
            : 'btn-ghost border border-border'}"
        >
          <Icon name={muted ? "micOff" : "mic"} size={17} />
        </button>
        <button
          onclick={endCall}
          disabled={phase === "connecting"}
          title="Kết thúc"
          class="px-4 h-11 rounded-full flex items-center justify-center text-white disabled:opacity-40"
          style="background: var(--color-danger);"
        >
          <Icon name="phone" size={17} class="rotate-[135deg]" />
        </button>
        <button
          onclick={toggleHeadphones}
          title={headphones
            ? "Đang dùng tai nghe — nói chen ngang được"
            : "Đang dùng loa ngoài — app tạm tắt micro lúc AI nói để tránh vọng tiếng"}
          class="h-11 px-3 rounded-full flex items-center gap-1.5 text-[11px] font-medium transition-colors {headphones
            ? 'btn-accent'
            : 'btn-ghost border border-border'}"
        >
          <Icon name="volume" size={14} />
          {headphones ? "Tai nghe" : "Loa ngoài"}
        </button>
      {/if}
    </div>
  {/if}
</div>
