<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Icon from "$lib/Icon.svelte";
  import { QUICK_PROMPTS, PROMPT_EXPLAIN, type QuickPrompt } from "$lib/config";
  import { currentModel, loadSettings } from "$lib/settings";
  import { askAIStream, type ChatTurn } from "$lib/aiClient";
  import { renderMarkdown, markdownToPlainText } from "$lib/markdown";

  type Phase = "ask" | "chat";

  let phase = $state<Phase>("ask");
  let cropB64 = $state("");
  let question = $state("");
  let history = $state<ChatTurn[]>([]);
  let busy = $state(false);
  let followupText = $state("");
  let copyFlash = $state(false);
  let error = $state("");
  let statusLine = $state("");
  let elapsedSec = $state(0);
  let modelLabel = $state("");
  let showImagePreview = $state(false);

  // Hiệu ứng "reveal" chữ giống ChatGPT/Claude: tách từng đoạn AI trả về thành
  // các mẩu nhỏ (từ + khoảng trắng), mỗi mẩu fade-in riêng thay vì bật cả cục.
  interface Chunk {
    id: number;
    text: string;
  }
  let streamChunks = $state<Chunk[]>([]);
  let chunkIdSeq = 0;

  function pushStreamPiece(piece: string) {
    const parts = piece.match(/\S+\s*|\s+/g) ?? [piece];
    for (const p of parts) streamChunks.push({ id: chunkIdSeq++, text: p });
  }

  let transcriptEl = $state<HTMLDivElement | undefined>();

  /** `silent`: KHÔNG hiện lỗi nếu ảnh chưa có — dùng cho lần thử đầu tiên lúc
   * mới mount, vì giờ cửa sổ này được mở NGAY (trước khi ảnh xử lý xong) để
   * người dùng thấy phản hồi tức thì, nên có thể ảnh CHƯA kịp nạp vào
   * `crop_sessions` phía Rust — đó là chuyện bình thường, không phải lỗi.
   * Ảnh sẽ tự nạp lại khi nhận event "ai:crop-ready" (xem onMount bên dưới). */
  async function loadCropImage(silent: boolean) {
    try {
      cropB64 = await invoke<string>("get_crop_image_base64", {
        windowLabel: getCurrentWindow().label,
      });
    } catch (e) {
      if (!silent) error = String(e);
    }
  }

  onMount(() => {
    // Cửa sổ này luôn được TẠO MỚI mỗi lần snip (xem commands.rs), nên onMount
    // chạy fresh mỗi lần — không cần lắng nghe event reset.
    //
    // Cửa sổ giờ mở NGAY khi vừa chọn xong vùng (trước khi crop/resize ảnh
    // xong) để phản hồi tức thì thay vì "chờ mù" — nên thử nạp ảnh ngay (silent,
    // phòng trường hợp ảnh đã kịp xử lý xong), đồng thời lắng nghe event
    // "ai:crop-ready" từ Rust để nạp lại khi ảnh THẬT SỰ sẵn sàng.
    let unlisten: (() => void) | undefined;
    loadCropImage(true);
    listen("ai:crop-ready", () => loadCropImage(false)).then((fn) => (unlisten = fn));
    const s = loadSettings();
    modelLabel = currentModel(s);
    return () => unlisten?.();
  });

  async function scrollToBottom() {
    await tick();
    transcriptEl?.scrollTo({ top: transcriptEl.scrollHeight, behavior: "smooth" });
  }

  async function runTurn() {
    busy = true;
    streamChunks = [];
    error = "";
    await scrollToBottom();

    const startedAt = Date.now();
    elapsedSec = 0;
    const timer = setInterval(() => {
      elapsedSec = Math.round((Date.now() - startedAt) / 1000);
    }, 1000);

    try {
      const settings = loadSettings();
      const answer = await askAIStream(
        history,
        settings,
        (piece) => {
          pushStreamPiece(piece);
          scrollToBottom();
        },
        (s) => (statusLine = s),
      );
      history = [...history, { role: "assistant", content: answer }];
    } catch (e) {
      // Luôn hiện lỗi + không bao giờ để `busy` treo mãi (bug đã gặp trước đây:
      // UI đứng im ở trạng thái đang chờ mà không báo gì).
      error = String(e).replace(/^Error:\s*/, "");
    } finally {
      clearInterval(timer);
      statusLine = "";
      streamChunks = [];
      busy = false;
      await scrollToBottom();
    }
  }

  async function handleAsk() {
    const q = question.trim() || PROMPT_EXPLAIN;
    history = [{ role: "user", content: q }];
    phase = "chat";
    await scrollToBottom();
    runTurn();
  }

  /** Gửi thẳng 1 chip gợi ý — bong bóng chat hiện `chip.chatLabel` ngắn gọn,
   * còn `chip.prompt` (đầy đủ, chi tiết) mới là thứ thực sự gửi cho AI. */
  async function askWithPrompt(chip: QuickPrompt) {
    history = [{ role: "user", content: chip.prompt, displayLabel: chip.chatLabel }];
    phase = "chat";
    await scrollToBottom();
    runTurn();
  }

  function handleSend() {
    const q = followupText.trim();
    if (!q || busy) return;
    followupText = "";
    history = [...history, { role: "user", content: q }];
    scrollToBottom();
    runTurn();
  }

  async function handleCopy() {
    // Nút Copy ở header: chép câu trả lời AI GẦN NHẤT — tiện khi chỉ có 1 lượt
    // hỏi hoặc chỉ quan tâm kết quả mới nhất. Muốn chép 1 câu trả lời CŨ hơn
    // (khi đã hỏi tiếp nhiều lần) thì dùng nút copy nhỏ trên từng bong bóng.
    const lastAi = [...history].reverse().find((t) => t.role === "assistant");
    if (!lastAi) return;
    try {
      // Chép văn bản SẠCH (không dính cú pháp Markdown thô) — người dùng dán
      // thẳng vào email/Word/Zalo, xem markdownToPlainText() để hiểu vì sao.
      await writeText(markdownToPlainText(lastAi.content));
      copyFlash = true;
      setTimeout(() => (copyFlash = false), 1600);
    } catch (e) {
      error = String(e);
    }
  }

  // Copy TỪNG câu trả lời riêng lẻ, không chỉ câu cuối — theo dõi index vừa
  // copy để hiện tick ✓ đúng bong bóng đó (không phải hiện chung cho cả UI).
  let copiedTurnIndex = $state<number | null>(null);
  async function handleCopyTurn(i: number, content: string) {
    try {
      await writeText(markdownToPlainText(content));
      copiedTurnIndex = i;
      setTimeout(() => {
        if (copiedTurnIndex === i) copiedTurnIndex = null;
      }, 1600);
    } catch (e) {
      error = String(e);
    }
  }

  function handleClose() {
    getCurrentWindow().close();
  }

  function onPreviewKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") showImagePreview = false;
  }
</script>

<svelte:window onkeydown={showImagePreview ? onPreviewKeydown : undefined} />

<div class="app-bg h-screen flex flex-col text-text overflow-hidden">
  {#if phase === "ask"}
    <!-- ── Giai đoạn 1: xem ảnh + đặt câu hỏi ── -->
    <div class="flex-1 min-h-0 p-3 pb-0 flex items-center justify-center">
      {#if cropB64}
        <img
          src={`data:image/png;base64,${cropB64}`}
          alt="Vùng đã chụp"
          class="max-w-full max-h-full object-contain rounded-xl border border-border shadow-lg"
          transition:fade={{ duration: 180 }}
        />
      {:else if !error}
        <!-- Cửa sổ mở ngay khi vừa chọn xong vùng, ảnh còn đang xử lý (resize/
        encode) ở backend — hiện loading thay vì để khoảng trống im lặng. -->
        <div class="flex flex-col items-center gap-2 text-text-muted" transition:fade={{ duration: 140 }}>
          <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
          <span class="text-[11px]">Đang xử lý ảnh…</span>
        </div>
      {/if}
    </div>

    <div class="shrink-0 px-3 pt-3 flex flex-wrap gap-1.5">
      {#each QUICK_PROMPTS as chip (chip.label)}
        <button class="chip disabled:opacity-40" disabled={!cropB64} onclick={() => askWithPrompt(chip)}>
          <Icon name={chip.icon} size={13} />
          {chip.label}
        </button>
      {/each}
    </div>

    <div class="shrink-0 p-3 flex gap-2">
      <input
        type="text"
        bind:value={question}
        disabled={!cropB64}
        placeholder={cropB64 ? "Hỏi bất kỳ điều gì về vùng đã chụp…" : "Đang xử lý ảnh…"}
        class="field selectable flex-1 disabled:opacity-50"
        onkeydown={(e) => e.key === "Enter" && handleAsk()}
      />
      <button
        onclick={handleAsk}
        disabled={!cropB64}
        class="btn-accent px-4 rounded-lg text-[13px] flex items-center gap-1.5 disabled:opacity-40"
      >
        <Icon name="send" size={15} strokeWidth={2.2} />
      </button>
    </div>
  {:else}
    <!-- ── Giai đoạn 2: hội thoại ── -->
    <div class="glass shrink-0 h-12 flex items-center px-3 gap-2.5">
      {#if cropB64}
        <button
          type="button"
          onclick={() => (showImagePreview = true)}
          class="shrink-0 w-8 h-8 rounded-lg overflow-hidden border border-border hover:border-accent/60 transition-colors relative group"
          title="Xem lại ảnh đã chụp"
        >
          <img src={`data:image/png;base64,${cropB64}`} alt="Vùng đã chụp" class="w-full h-full object-cover" />
          <span
            class="absolute inset-0 bg-black/0 group-hover:bg-black/35 flex items-center justify-center transition-colors"
          >
            <Icon name="eye" size={13} class="text-white opacity-0 group-hover:opacity-100 transition-opacity" />
          </span>
        </button>
      {:else}
        <div
          class="w-6 h-6 rounded-lg flex items-center justify-center text-accent-text shrink-0 {busy ? 'pulse-ring' : ''}"
          style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
        >
          <Icon name="sparkles" size={13} strokeWidth={2.3} />
        </div>
      {/if}
      <div class="flex-1 min-w-0">
        <div class="text-[12.5px] font-semibold leading-tight">Kết quả AI</div>
        {#if modelLabel}
          <div class="text-[10.5px] text-text-muted leading-tight truncate font-mono">{modelLabel}</div>
        {/if}
      </div>
      <button
        onclick={handleCopy}
        disabled={busy}
        class="btn-ghost px-2.5 py-1.5 rounded-lg text-[11.5px] flex items-center gap-1.5 disabled:opacity-40"
        title="Sao chép câu trả lời"
      >
        <Icon name={copyFlash ? "check" : "copy"} size={13} />
        {copyFlash ? "Đã chép" : "Chép"}
      </button>
      <button onclick={handleClose} class="btn-ghost p-1.5 rounded-lg" title="Đóng">
        <Icon name="x" size={14} />
      </button>
    </div>

    {#if busy}
      <div class="shrink-0 h-0.5 shimmer" transition:fade={{ duration: 120 }}></div>
    {/if}

    <div bind:this={transcriptEl} class="selectable flex-1 min-h-0 overflow-y-auto px-3 py-3.5 flex flex-col gap-3.5">
      {#each history as turn, i (i)}
        {#if turn.role === "user"}
          <div class="flex justify-end msg-in">
            <div
              class="max-w-[86%] rounded-2xl rounded-br-md px-3.5 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap text-accent-text font-medium"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              {turn.displayLabel ?? turn.content}
            </div>
          </div>
        {:else}
          <div class="flex items-start gap-2.5 msg-in group">
            <div
              class="shrink-0 w-6 h-6 mt-0.5 rounded-lg flex items-center justify-center text-accent-text"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              <Icon name="sparkles" size={12} strokeWidth={2.3} />
            </div>
            <div class="relative max-w-[88%]">
              <div class="markdown-body card rounded-2xl rounded-tl-md px-3.5 py-2.5 pr-8 text-[12.5px]">
                {@html renderMarkdown(turn.content)}
              </div>
              <button
                onclick={() => handleCopyTurn(i, turn.content)}
                class="absolute top-1.5 right-1.5 p-1.5 rounded-md text-text-muted hover:text-accent hover:bg-white/8 opacity-0 group-hover:opacity-100 transition-opacity"
                title="Sao chép câu trả lời này"
              >
                <Icon name={copiedTurnIndex === i ? "check" : "copy"} size={12} />
              </button>
            </div>
          </div>
        {/if}
      {/each}

      {#if busy}
        <div class="flex items-start gap-2.5">
          <div
            class="shrink-0 w-6 h-6 mt-0.5 rounded-lg flex items-center justify-center text-accent-text pulse-ring"
            style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
          >
            <Icon name="sparkles" size={12} strokeWidth={2.3} />
          </div>
          <div class="card max-w-[88%] rounded-2xl rounded-tl-md px-3.5 py-2.5 text-[12.5px] leading-relaxed">
            {#if streamChunks.length === 0}
              <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
            {:else}
              <span class="whitespace-pre-wrap"
                >{#each streamChunks as chunk (chunk.id)}<span in:fade={{ duration: 170 }}>{chunk.text}</span
                  >{/each}<span class="typing-cursor"></span></span
              >
            {/if}
          </div>
        </div>
      {/if}

      {#if error}
        <div
          class="card border-[color:var(--color-danger)]/40 px-3.5 py-2.5 text-[12px] text-[color:var(--color-danger)] flex items-start gap-2 msg-in"
        >
          <Icon name="alert" size={14} class="shrink-0 mt-0.5" />
          <span class="selectable leading-relaxed">{error}</span>
        </div>
      {/if}
    </div>

    <div class="shrink-0 px-3 pb-3 pt-1 flex flex-col gap-1.5">
      {#if busy && statusLine}
        <div class="text-[10.5px] text-text-muted flex items-center gap-1.5 px-0.5" transition:fade={{ duration: 140 }}>
          <span class="truncate">{statusLine}</span>
          <span class="text-accent font-semibold shrink-0">{elapsedSec}s</span>
        </div>
      {/if}
      <div class="flex gap-2">
        <input
          type="text"
          bind:value={followupText}
          disabled={busy}
          placeholder={busy ? "AI đang trả lời…" : "Hỏi tiếp…"}
          class="field selectable flex-1 disabled:opacity-50"
          onkeydown={(e) => e.key === "Enter" && handleSend()}
        />
        <button onclick={handleSend} disabled={busy} class="btn-accent px-4 rounded-lg">
          <Icon name="send" size={15} strokeWidth={2.2} />
        </button>
      </div>
    </div>
  {/if}

  {#if showImagePreview && cropB64}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-40 bg-black/80 backdrop-blur-sm flex items-center justify-center p-6"
      role="presentation"
      onclick={() => (showImagePreview = false)}
      transition:fade={{ duration: 140 }}
    >
      <img
        src={`data:image/png;base64,${cropB64}`}
        alt="Vùng đã chụp (phóng to)"
        class="max-w-full max-h-full object-contain rounded-xl border border-border shadow-2xl"
      />
      <button
        onclick={() => (showImagePreview = false)}
        class="absolute top-4 right-4 btn-ghost p-2 rounded-lg"
        title="Đóng (Esc)"
      >
        <Icon name="x" size={18} />
      </button>
    </div>
  {/if}
</div>
