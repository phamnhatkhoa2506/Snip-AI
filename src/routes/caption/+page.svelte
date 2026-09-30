<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import Icon from "$lib/Icon.svelte";
  import ScrollArea from "$lib/ScrollArea.svelte";
  import { saveBytesAs } from "$lib/exportImage";
  import {
    allowedAudioSources,
    CAPTION_LANG_OPTIONS,
    loadSettings,
    saveSettings,
    MIC_PERMISSION_ERROR_PREFIX,
    type CaptionMode,
    type AudioSnapSource,
  } from "$lib/settings";

  // Phụ đề trực tiếp: cửa sổ luôn nằm trên cùng, hiện chữ của âm thanh máy /
  // micro theo thời gian thực để người dùng vừa làm việc khác vừa nghe vừa
  // đọc. Thu âm + WebSocket chạy ở Rust (caption.rs); trang này chỉ hiển thị.

  type Phase = "idle" | "connecting" | "listening" | "reconnecting";
  let phase = $state<Phase>("idle");
  let error = $state("");
  let notice = $state("");
  let windowsBlocked = $state(false);
  let permissionBusy = $state(false);
  let paused = $state(false);
  let level = $state(0);

  const initial = loadSettings();
  let mode = $state<CaptionMode>(initial.captionMode);
  let source = $state<AudioSnapSource>(initial.captionSource);
  let targetLang = $state(initial.captionTargetLang);
  let fontSize = $state(initial.captionFontSize);

  interface Line {
    /** Chép lời: câu đã chốt. Dịch: bản dịch. */
    text: string;
    /** Chỉ chế độ dịch: chữ gốc. */
    src: string;
    done: boolean;
  }
  let lines = $state<Line[]>([]);
  let interim = $state("");
  let viewport = $state<HTMLDivElement | null>(null);
  let stick = true;

  /** Quyền có thể vừa được bật -> đọc lại settings mỗi lần tick đổi. */
  let settingsTick = $state(0);
  function currentSettings() {
    settingsTick;
    return loadSettings();
  }

  const running = $derived(phase !== "idle");
  const needsPermission = $derived(allowedAudioSources(currentSettings(), source).length === 0);

  function persist() {
    saveSettings({
      ...loadSettings(),
      captionMode: mode,
      captionSource: source,
      captionTargetLang: targetLang,
      captionFontSize: fontSize,
    });
  }

  async function follow() {
    if (!stick) return;
    await tick();
    viewport?.scrollTo({ top: viewport.scrollHeight });
  }

  $effect(() => {
    const el = viewport;
    if (!el) return;
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  });

  function onScroll() {
    if (!viewport) return;
    stick = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 40;
  }

  function openLine(): Line {
    const last = lines[lines.length - 1];
    if (last && !last.done) return last;
    lines.push({ text: "", src: "", done: false });
    return lines[lines.length - 1];
  }

  function onChunk(p: { channel: string; text: string }) {
    if (p.channel === "interim") {
      interim = p.text;
    } else if (p.channel === "final") {
      interim = "";
      const t = p.text.trim();
      if (t) lines.push({ text: t, src: "", done: true });
    } else if (p.channel === "source") {
      openLine().src += p.text;
    } else if (p.channel === "target") {
      openLine().text += p.text;
    }
    follow();
  }

  async function start() {
    error = "";
    notice = "";
    windowsBlocked = false;
    const sources = allowedAudioSources(loadSettings(), source);
    if (sources.length === 0) {
      settingsTick++;
      return;
    }
    persist();
    phase = "connecting";
    paused = false;
    interim = "";
    try {
      await invoke("caption_start", { mode, sources, targetLang });
      if (phase === "connecting") phase = "listening";
    } catch (e) {
      const msg = String(e);
      windowsBlocked = msg.includes(MIC_PERMISSION_ERROR_PREFIX);
      error = msg.replace(MIC_PERMISSION_ERROR_PREFIX, "");
      phase = "idle";
    }
  }

  function stop() {
    invoke("caption_stop");
  }

  function togglePause() {
    paused = !paused;
    invoke("caption_set_paused", { paused });
  }

  async function allowMic() {
    permissionBusy = true;
    error = "";
    windowsBlocked = false;
    try {
      await invoke("probe_microphone");
      saveSettings({ ...loadSettings(), micAllowed: true });
      settingsTick++;
    } catch (e) {
      const msg = String(e);
      windowsBlocked = msg.includes(MIC_PERMISSION_ERROR_PREFIX);
      error = msg.replace(MIC_PERMISSION_ERROR_PREFIX, "");
    } finally {
      permissionBusy = false;
    }
  }

  function allowSystem() {
    saveSettings({ ...loadSettings(), systemAudioAllowed: true });
    settingsTick++;
  }

  /** Văn bản xuất/chép: chép lời = mỗi câu 1 dòng; dịch = bản dịch, dòng
   * dưới là chữ gốc. */
  function asText(): string {
    const parts = lines.map((l) =>
      mode === "translate" && l.src.trim() ? `${l.text.trim()}\n  (${l.src.trim()})` : l.text.trim(),
    );
    if (interim.trim()) parts.push(interim.trim());
    return parts.filter(Boolean).join("\n");
  }

  async function copyAll() {
    try {
      await navigator.clipboard.writeText(asText());
      notice = "Đã chép vào clipboard.";
    } catch {
      notice = "Không chép được vào clipboard.";
    }
  }

  async function exportTxt() {
    try {
      const bytes = new TextEncoder().encode(asText() + "\n");
      const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, "-");
      const ok = await saveBytesAs(bytes.buffer as ArrayBuffer, `phu-de-${stamp}.txt`, [
        { name: "Văn bản", extensions: ["txt"] },
      ]);
      if (ok) notice = "Đã lưu file.";
    } catch (e) {
      error = String(e);
    }
  }

  function clearAll() {
    lines = [];
    interim = "";
  }

  onMount(() => {
    const unlistens: UnlistenFn[] = [];
    const on = <T,>(event: string, handler: (payload: T) => void) =>
      listen<T>(event, (e) => handler(e.payload)).then((fn) => unlistens.push(fn));
    Promise.all([
      on<string>("caption:state", (s) => {
        if (s === "listening" || s === "reconnecting") phase = s;
      }),
      on<number>("caption:level", (l) => (level = l)),
      on<{ channel: string; text: string }>("caption:chunk", onChunk),
      on<null>("caption:break", () => {
        for (const l of lines) l.done = true;
      }),
      on<string>("caption:ended", (reason) => {
        for (const l of lines) l.done = true;
        interim = "";
        level = 0;
        phase = "idle";
        paused = false;
        if (reason) error = reason;
      }),
    ]).catch((e) => {
      error = `Không khởi tạo được cửa sổ phụ đề: ${e}`;
    });
    return () => {
      unlistens.forEach((fn) => fn());
      invoke("caption_stop").catch(() => {});
    };
  });

  const statusText = $derived(
    phase === "connecting"
      ? "Đang kết nối…"
      : phase === "reconnecting"
        ? "Đang nối lại…"
        : phase === "listening"
          ? paused
            ? "Đã tạm dừng"
            : mode === "translate"
              ? "Đang nghe và dịch"
              : "Đang nghe"
          : "Chưa bật",
  );

  const SOURCE_LABEL: Record<AudioSnapSource, string> = {
    system: "Âm thanh máy",
    mic: "Micro",
    both: "Máy + micro",
  };
</script>

<div class="app-bg h-screen flex flex-col text-text overflow-hidden">
  <!-- Thanh cấu hình — khoá lúc đang chạy (đổi chế độ/nguồn phải nối lại phiên). -->
  <div class="shrink-0 flex flex-wrap items-center gap-2 px-3 pt-2.5 pb-2 border-b border-border">
    <div class="flex rounded-full p-0.5" style="background: var(--color-card); border: 1px solid var(--color-border);">
      {#each [["transcribe", "Chép lời"], ["translate", "Dịch"]] as [value, label] (value)}
        <button
          onclick={() => {
            mode = value as CaptionMode;
            persist();
          }}
          disabled={running}
          class="px-3 h-7 rounded-full text-[11.5px] font-medium transition-colors disabled:cursor-default {mode === value
            ? 'btn-accent'
            : 'text-text-muted hover:text-text'}"
        >
          {label}
        </button>
      {/each}
    </div>
    <select
      bind:value={source}
      onchange={persist}
      disabled={running}
      class="h-7 rounded-lg px-2 text-[11.5px] card disabled:opacity-60"
      aria-label="Nguồn âm thanh"
    >
      {#each Object.entries(SOURCE_LABEL) as [value, label] (value)}
        <option {value}>{label}</option>
      {/each}
    </select>
    {#if mode === "translate"}
      <select
        bind:value={targetLang}
        onchange={persist}
        disabled={running}
        class="h-7 rounded-lg px-2 text-[11.5px] card disabled:opacity-60"
        aria-label="Dịch sang"
      >
        {#each CAPTION_LANG_OPTIONS as o (o.value)}
          <option value={o.value}>Dịch sang {o.title}</option>
        {/each}
      </select>
    {/if}
    <div class="ml-auto flex items-center gap-0.5">
      <button
        onclick={() => {
          fontSize = Math.max(12, fontSize - 2);
          persist();
        }}
        class="w-7 h-7 rounded-lg text-[11px] text-text-muted hover:text-text"
        title="Chữ nhỏ hơn"
        aria-label="Chữ nhỏ hơn">A-</button
      >
      <button
        onclick={() => {
          fontSize = Math.min(36, fontSize + 2);
          persist();
        }}
        class="w-7 h-7 rounded-lg text-[13px] font-semibold text-text-muted hover:text-text"
        title="Chữ to hơn"
        aria-label="Chữ to hơn">A+</button
      >
    </div>
  </div>

  {#if !running && needsPermission}
    <div class="flex-1 flex flex-col items-center justify-center gap-3 px-6 text-center">
      <p class="text-[12.5px] text-text-muted leading-relaxed max-w-[340px]">
        {source === "mic"
          ? "Phụ đề từ micro cần bạn cho phép app dùng micro."
          : source === "system"
            ? "Phụ đề từ âm thanh máy cần bạn cho phép app thu mọi âm thanh đang phát ra loa."
            : "Cần cho phép cả micro và âm thanh máy."}
      </p>
      {#if (source === "mic" || source === "both") && !currentSettings().micAllowed}
        {#if windowsBlocked}
          <button
            onclick={() => invoke("open_mic_privacy_settings")}
            class="btn-accent px-4 py-2 rounded-lg text-[12.5px] font-semibold"
          >
            Mở cài đặt quyền micro của Windows
          </button>
        {/if}
        <button
          onclick={allowMic}
          disabled={permissionBusy}
          class="btn-accent px-4 py-2 rounded-lg text-[12.5px] font-semibold flex items-center gap-1.5 disabled:opacity-60"
        >
          <Icon name={permissionBusy ? "loader" : "mic"} size={13} class={permissionBusy ? "animate-spin" : ""} />
          Cho phép dùng micro
        </button>
      {/if}
      {#if (source === "system" || source === "both") && !currentSettings().systemAudioAllowed}
        <button
          onclick={allowSystem}
          class="btn-accent px-4 py-2 rounded-lg text-[12.5px] font-semibold flex items-center gap-1.5"
        >
          <Icon name="volume" size={13} /> Cho phép thu âm thanh máy
        </button>
      {/if}
      {#if error}
        <p class="text-[11.5px] text-[color:var(--color-danger)] leading-relaxed selectable">{error}</p>
      {/if}
    </div>
  {:else}
    <ScrollArea
      bind:viewport
      class="selectable flex-1 min-h-0"
      contentClass="px-4 py-3 flex flex-col gap-2"
    >
      {#if lines.length === 0 && !interim}
        <p class="text-[12px] text-text-muted text-center mt-6 leading-relaxed">
          {#if running}
            Đang chờ âm thanh… Phát video/âm thanh trên máy, chữ sẽ hiện ở đây.
          {:else}
            Bấm “Bắt đầu”, rồi chuyển sang việc khác — cửa sổ này luôn nằm trên cùng.
          {/if}
        </p>
      {/if}
      {#each lines as line, i (i)}
        <div class="leading-snug" style="font-size: {fontSize}px;">
          <div>{line.text}</div>
          {#if mode === "translate" && line.src.trim()}
            <div class="text-text-muted mt-0.5" style="font-size: {Math.max(11, fontSize - 4)}px;">
              {line.src}
            </div>
          {/if}
        </div>
      {/each}
      {#if interim}
        <div class="leading-snug text-text-muted italic" style="font-size: {fontSize}px;">{interim}</div>
      {/if}
      {#if error}
        <div class="card px-3 py-2 text-[12px] text-[color:var(--color-danger)] flex items-start gap-2">
          <Icon name="alert" size={14} class="shrink-0 mt-0.5" />
          <span class="leading-relaxed">{error}</span>
        </div>
      {/if}
    </ScrollArea>
  {/if}

  <div class="shrink-0 flex items-center gap-1.5 px-3 py-2 border-t border-border">
    {#if running}
      <button
        onclick={stop}
        class="h-8 px-3 rounded-full text-[12px] font-semibold text-white flex items-center gap-1.5"
        style="background: var(--color-danger);"
      >
        <Icon name="stopSquare" size={12} /> Dừng
      </button>
      <button
        onclick={togglePause}
        disabled={phase !== "listening"}
        class="h-8 px-3 rounded-full text-[12px] font-medium disabled:opacity-40 {paused
          ? 'btn-accent'
          : 'btn-ghost border border-border'}"
      >
        {paused ? "Tiếp tục" : "Tạm dừng"}
      </button>
    {:else}
      <button
        onclick={start}
        disabled={needsPermission}
        class="btn-accent h-8 px-4 rounded-full text-[12px] font-semibold flex items-center gap-1.5 disabled:opacity-50"
      >
        <Icon name="captions" size={14} /> Bắt đầu
      </button>
    {/if}
    <div class="flex items-center gap-1.5 ml-1 min-w-0">
      <span
        class="shrink-0 w-2 h-2 rounded-full transition-transform duration-100"
        style="background: {phase === 'listening' && !paused
          ? 'var(--color-accent)'
          : 'var(--color-text-muted)'}; transform: scale({1 + Math.min(level, 1) * 1.2});"
      ></span>
      <span class="text-[11px] text-text-muted truncate">{notice || statusText}</span>
    </div>
    <div class="ml-auto flex items-center">
      <button
        onclick={copyAll}
        disabled={lines.length === 0 && !interim}
        class="w-8 h-8 rounded-lg flex items-center justify-center text-text-muted hover:text-text disabled:opacity-30"
        title="Chép toàn bộ"
        aria-label="Chép toàn bộ"><Icon name="copy" size={14} /></button
      >
      <button
        onclick={exportTxt}
        disabled={lines.length === 0}
        class="w-8 h-8 rounded-lg flex items-center justify-center text-text-muted hover:text-text disabled:opacity-30"
        title="Lưu ra file .txt"
        aria-label="Lưu ra file .txt"><Icon name="download" size={14} /></button
      >
      <button
        onclick={clearAll}
        disabled={lines.length === 0 && !interim}
        class="w-8 h-8 rounded-lg flex items-center justify-center text-text-muted hover:text-text disabled:opacity-30"
        title="Xoá màn hình"
        aria-label="Xoá màn hình"><Icon name="trash" size={14} /></button
      >
    </div>
  </div>
</div>
