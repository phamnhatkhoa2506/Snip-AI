<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import Icon from "$lib/Icon.svelte";
  import { QUICK_PROMPTS, VIDEO_PROMPTS, PROMPT_EXPLAIN, PROMPT_VIDEO_EXPLAIN, type QuickPrompt } from "$lib/config";
  import { currentModel, loadSettings } from "$lib/settings";
  import { askAIStream, type ChatTurn } from "$lib/aiClient";
  import { renderMarkdown, markdownToPlainText } from "$lib/markdown";

  type Phase = "ask" | "chat";

  // Cửa sổ này dùng chung 1 route cho CẢ 2 loại phiên: snip ảnh (label
  // "result-N") và quay video (label "record-N") — tự nhận biết qua tiền tố
  // label, xem commands.rs (RESULT_LABEL_PREFIX/RECORD_LABEL_PREFIX). Khác
  // ảnh: cửa sổ video chỉ được TẠO SAU KHI quay xong (xem
  // record.rs::start_recording) — video LUÔN đã có sẵn trong `video_sessions`
  // ngay từ lúc cửa sổ này mount, không cần chờ/nghe event gì thêm.
  const isVideoSession = getCurrentWindow().label.startsWith("record-");

  // Bộ chip gợi ý khác nhau giữa 2 chế độ. Không chỉ là đổi chữ "ảnh" thành
  // "video": video có trục thời gian và bị Gemini lấy mẫu thưa (~1 khung/giây)
  // nên câu hỏi phải đặt khác hẳn — xem giải thích đầy đủ ở config.ts, ngay
  // trên PROMPT_VIDEO_OCR.
  const quickPrompts = isVideoSession ? VIDEO_PROMPTS : QUICK_PROMPTS;

  let phase = $state<Phase>("ask");
  /** Base64 của ảnh (PNG) hoặc video (MP4) tuỳ loại phiên — tên chung
   * "mediaB64" thay vì "cropB64" vì giờ không còn chỉ là ảnh crop nữa. */
  let mediaB64 = $state("");
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

  // ── Chỉ định giây/khoảng để hỏi (chỉ áp dụng cho phiên VIDEO) ───────────
  // Không cắt file video thật (đã kiểm chứng field `video_metadata` của
  // Gemini bị model mới lờ đi — xem ghi chú trong ai.rs). Thay vào đó gửi
  // NGUYÊN video như cũ, chỉ thêm 1 câu chỉ dẫn bằng lời vào cuối câu hỏi.
  // Đã kiểm chứng thực tế: hỏi thẳng "tại giây thứ N có gì" trên video KHÔNG
  // cắt vẫn cho Gemini trả lời đúng tuyệt đối (model tự gắn timestamp theo
  // từng khung lấy mẫu) — rẻ hơn nhiều so với tự dựng bộ cắt MP4 mà vẫn đáng
  // tin cậy ngang nhau.
  let videoEl = $state<HTMLVideoElement | null>(null);
  let videoDuration = $state(0);
  let rangeStart = $state(0);
  let rangeEnd = $state(0);
  /** false = chưa động vào thanh chọn -> không giới hạn gì, hỏi cả video. */
  let rangeTouched = $state(false);

  function formatClock(totalSec: number): string {
    const s = Math.max(0, Math.round(totalSec));
    const m = Math.floor(s / 60);
    const ss = String(s % 60).padStart(2, "0");
    return `${m}:${ss}`;
  }

  /** true nếu 2 tay cầm gần như trùng nhau -> coi là "1 thời điểm" thay vì
   * "1 khoảng" (chênh dưới 1s, khó kéo trùng tuyệt đối bằng chuột). */
  const isTimePoint = $derived(rangeEnd - rangeStart < 1);

  const timeRangeLabel = $derived(
    isTimePoint ? formatClock(rangeStart) : `${formatClock(rangeStart)}–${formatClock(rangeEnd)}`,
  );

  /** Câu chỉ dẫn nối vào CUỐI nội dung thật gửi cho AI (không hiện lên bong
   * bóng chat) khi người dùng đã chỉ định thời điểm/khoảng. Rỗng nếu chưa
   * chỉnh gì -> hành vi giữ nguyên như trước (hỏi cả video). */
  const timeContextSuffix = $derived(
    !rangeTouched
      ? ""
      : isTimePoint
        ? `\n\n(Chỉ tập trung vào đúng thời điểm ${formatClock(rangeStart)} trong video, không phải toàn bộ video.)`
        : `\n\n(Chỉ tập trung vào đoạn video từ ${formatClock(rangeStart)} đến ${formatClock(rangeEnd)}, không phải toàn bộ video.)`,
  );

  function onVideoLoadedMetadata() {
    if (!videoEl || !Number.isFinite(videoEl.duration)) return;
    videoDuration = videoEl.duration;
    rangeEnd = videoEl.duration;
  }

  /** Kéo tay cầm thì tua luôn video xem trước đúng thời điểm đó — cho người
   * dùng thấy ngay "giây này có gì" thay vì phải đoán bằng số giây suông. */
  function seekPreview(t: number) {
    if (videoEl) videoEl.currentTime = t;
  }

  function clearTimeRange() {
    rangeTouched = false;
    rangeStart = 0;
    rangeEnd = videoDuration;
  }

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

  /** `silent`: KHÔNG hiện lỗi nếu ảnh/video chưa có — dùng cho lần thử đầu
   * tiên lúc mới mount, vì cửa sổ này được mở NGAY (trước khi ảnh/video xử
   * lý xong) để phản hồi tức thì, nên có thể CHƯA kịp nạp vào
   * `crop_sessions`/`video_sessions` phía Rust — đó là chuyện bình thường,
   * không phải lỗi. Sẽ tự nạp lại khi nhận event "ai:crop-ready"/
   * "recording:ready" (xem onMount bên dưới). */
  async function loadMedia(silent: boolean) {
    try {
      mediaB64 = await invoke<string>(isVideoSession ? "get_recording_base64" : "get_crop_image_base64", {
        windowLabel: getCurrentWindow().label,
      });
    } catch (e) {
      if (!silent) error = String(e);
    }
  }

  onMount(() => {
    // Cửa sổ này luôn được TẠO MỚI mỗi lần snip/quay (xem commands.rs), nên
    // onMount chạy fresh mỗi lần — không cần lắng nghe event reset.
    let unlisten: (() => void) | undefined;
    const s = loadSettings();
    modelLabel = currentModel(s);

    if (isVideoSession) {
      // Phiên video: cửa sổ chỉ mở SAU KHI quay xong, video đã sẵn sàng ngay
      // từ đầu — nạp thẳng, không cần silent/event gì cả.
      loadMedia(false);
    } else {
      // Phiên ảnh: cửa sổ mở NGAY khi vừa chọn xong vùng (trước khi crop/resize
      // ảnh xong) để phản hồi tức thì thay vì "chờ mù" — nên thử nạp ảnh ngay
      // (silent, phòng trường hợp ảnh đã kịp xử lý xong), đồng thời lắng nghe
      // event "ai:crop-ready" từ Rust để nạp lại khi ảnh THẬT SỰ sẵn sàng.
      loadMedia(true);
      listen("ai:crop-ready", () => loadMedia(false)).then((fn) => (unlisten = fn));
    }

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

  /** Hậu tố "(00:05)" / "(00:05–00:12)" gắn vào displayLabel khi có chỉ định
   * thời điểm — để bong bóng chat TỰ ghi lại đã hỏi trong phạm vi nào, không
   * cần người dùng nhớ lại. Chỉ áp dụng phiên video. */
  const timeBadgeSuffix = $derived(isVideoSession && rangeTouched ? ` (${timeRangeLabel})` : "");

  async function handleAsk() {
    const typed = question.trim();
    const q = typed || (isVideoSession ? PROMPT_VIDEO_EXPLAIN : PROMPT_EXPLAIN);
    const displayLabel = timeBadgeSuffix ? `${typed || "Giải thích nội dung"}${timeBadgeSuffix}` : undefined;
    history = [{ role: "user", content: q + timeContextSuffix, displayLabel }];
    phase = "chat";
    await scrollToBottom();
    runTurn();
  }

  /** Gửi thẳng 1 chip gợi ý — bong bóng chat hiện `chip.chatLabel` ngắn gọn,
   * còn `chip.prompt` (đầy đủ, chi tiết) mới là thứ thực sự gửi cho AI. Có
   * chỉ định thời điểm thì nối thêm `timeContextSuffix` vào nội dung thật
   * (AI đọc) và `timeBadgeSuffix` vào nhãn hiển thị (người dùng thấy). */
  async function askWithPrompt(chip: QuickPrompt) {
    history = [
      { role: "user", content: chip.prompt + timeContextSuffix, displayLabel: chip.chatLabel + timeBadgeSuffix },
    ];
    phase = "chat";
    await scrollToBottom();
    runTurn();
  }

  function handleSend() {
    const q = followupText.trim();
    if (!q || busy) return;
    followupText = "";
    const displayLabel = timeBadgeSuffix ? `${q}${timeBadgeSuffix}` : undefined;
    history = [...history, { role: "user", content: q + timeContextSuffix, displayLabel }];
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
    <!-- ── Giai đoạn 1: xem ảnh/video + đặt câu hỏi ── -->
    <div class="flex-1 min-h-0 p-3 pb-0 flex items-center justify-center">
      {#if mediaB64}
        {#if isVideoSession}
          <video
            bind:this={videoEl}
            onloadedmetadata={onVideoLoadedMetadata}
            src={`data:video/mp4;base64,${mediaB64}`}
            controls
            autoplay
            muted
            loop
            class="max-w-full max-h-full rounded-xl border border-border shadow-lg"
            transition:fade={{ duration: 180 }}
          ></video>
        {:else}
          <img
            src={`data:image/png;base64,${mediaB64}`}
            alt="Vùng đã chụp"
            class="max-w-full max-h-full object-contain rounded-xl border border-border shadow-lg"
            transition:fade={{ duration: 180 }}
          />
        {/if}
      {:else if !error}
        <!-- Cửa sổ mở ngay khi vừa chọn xong vùng/quay xong, ảnh/video còn
        đang xử lý ở backend — hiện loading thay vì để khoảng trống im lặng. -->
        <div class="flex flex-col items-center gap-2 text-text-muted" transition:fade={{ duration: 140 }}>
          <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
          <span class="text-[11px]">{isVideoSession ? "Đang xử lý video…" : "Đang xử lý ảnh…"}</span>
        </div>
      {/if}
    </div>

    {#if isVideoSession && videoDuration > 0}
      <!-- Thanh chọn thời điểm/khoảng — 2 thanh <input type=range> chồng lên
      nhau (kỹ thuật dual-range kinh điển: mỗi input tự lo 1 tay cầm, CSS cho
      track trong suốt để chỉ thấy phần "đã tô" ở giữa). Kéo trùng 2 tay cầm
      thì thành "1 thời điểm" thay vì "1 khoảng" (xem isTimePoint). -->
      <div class="shrink-0 px-3 pt-2.5">
        <div class="flex items-center justify-between mb-1">
          <span class="text-[10.5px] text-text-muted flex items-center gap-1">
            <Icon name="target" size={11} />
            {rangeTouched ? `Đang hỏi về ${isTimePoint ? "thời điểm" : "khoảng"} ${timeRangeLabel}` : "Kéo để hỏi về 1 thời điểm/khoảng cụ thể"}
          </span>
          {#if rangeTouched}
            <button
              type="button"
              onclick={clearTimeRange}
              class="text-[10.5px] text-text-muted hover:text-text transition-colors flex items-center gap-0.5"
            >
              <Icon name="x" size={10} /> Bỏ chọn
            </button>
          {/if}
        </div>
        <div class="relative h-4 flex items-center">
          <div class="absolute inset-x-0 h-1 rounded-full bg-bg-elevated"></div>
          <div
            class="absolute h-1 rounded-full"
            style="left:{(rangeStart / videoDuration) * 100}%; right:{100 - (rangeEnd / videoDuration) * 100}%; background: linear-gradient(90deg, var(--color-accent), var(--color-accent-2));"
          ></div>
          <input
            type="range"
            min="0"
            max={videoDuration}
            step="0.1"
            value={rangeStart}
            class="range-thumb"
            oninput={(e) => {
              const v = Math.min(Number(e.currentTarget.value), rangeEnd);
              rangeStart = v;
              rangeTouched = true;
              seekPreview(v);
            }}
          />
          <input
            type="range"
            min="0"
            max={videoDuration}
            step="0.1"
            value={rangeEnd}
            class="range-thumb"
            oninput={(e) => {
              const v = Math.max(Number(e.currentTarget.value), rangeStart);
              rangeEnd = v;
              rangeTouched = true;
              seekPreview(v);
            }}
          />
        </div>
      </div>
    {/if}

    <div class="shrink-0 px-3 pt-3 flex flex-wrap gap-1.5">
      {#each quickPrompts as chip (chip.id)}
        <button class="chip disabled:opacity-40" disabled={!mediaB64} onclick={() => askWithPrompt(chip)}>
          <Icon name={chip.icon} size={13} />
          {chip.label}
        </button>
      {/each}
    </div>

    <div class="shrink-0 p-3 flex gap-2">
      <input
        type="text"
        bind:value={question}
        disabled={!mediaB64}
        placeholder={mediaB64
          ? `Hỏi bất kỳ điều gì về ${isVideoSession ? "video" : "vùng"} đã ${isVideoSession ? "quay" : "chụp"}…`
          : isVideoSession
            ? "Đang xử lý video…"
            : "Đang xử lý ảnh…"}
        class="field selectable flex-1 disabled:opacity-50"
        onkeydown={(e) => e.key === "Enter" && handleAsk()}
      />
      <button
        onclick={handleAsk}
        disabled={!mediaB64}
        class="btn-accent px-4 rounded-lg text-[13px] flex items-center gap-1.5 disabled:opacity-40"
      >
        <Icon name="send" size={15} strokeWidth={2.2} />
      </button>
    </div>
  {:else}
    <!-- ── Giai đoạn 2: hội thoại ── -->
    <div class="glass shrink-0 h-12 flex items-center px-3 gap-2.5">
      {#if mediaB64}
        <button
          type="button"
          onclick={() => (showImagePreview = true)}
          class="shrink-0 w-8 h-8 rounded-lg overflow-hidden border border-border hover:border-accent/60 transition-colors relative group"
          title={isVideoSession ? "Xem lại video đã quay" : "Xem lại ảnh đã chụp"}
        >
          {#if isVideoSession}
            <video src={`data:video/mp4;base64,${mediaB64}`} muted class="w-full h-full object-cover"></video>
          {:else}
            <img src={`data:image/png;base64,${mediaB64}`} alt="Vùng đã chụp" class="w-full h-full object-cover" />
          {/if}
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
      {#if isVideoSession && rangeTouched}
        <!-- Khoảng chọn vẫn còn hiệu lực cho câu hỏi tiếp theo (sticky) —
        nhắc lại ở đây để người dùng không quên đang giới hạn phạm vi hỏi. -->
        <div class="flex items-center gap-1.5 text-[10.5px] text-accent px-0.5">
          <Icon name="target" size={11} />
          Đang hỏi về {isTimePoint ? "thời điểm" : "khoảng"} {timeRangeLabel}
          <button type="button" onclick={clearTimeRange} class="text-text-muted hover:text-text transition-colors ml-0.5">
            <Icon name="x" size={10} />
          </button>
        </div>
      {/if}
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

  {#if showImagePreview && mediaB64}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-40 bg-black/80 backdrop-blur-sm flex items-center justify-center p-6"
      role="presentation"
      onclick={() => (showImagePreview = false)}
      transition:fade={{ duration: 140 }}
    >
      {#if isVideoSession}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_media_has_caption -->
        <!-- Video tự quay bằng app này KHÔNG có track âm thanh (đã tắt hẳn ở
        record.rs) nên không có gì để phụ đề — cảnh báo a11y này không áp dụng được. -->
        <video
          src={`data:video/mp4;base64,${mediaB64}`}
          controls
          autoplay
          onclick={(e) => e.stopPropagation()}
          class="max-w-full max-h-full rounded-xl border border-border shadow-2xl"
        ></video>
      {:else}
        <img
          src={`data:image/png;base64,${mediaB64}`}
          alt="Vùng đã chụp (phóng to)"
          class="max-w-full max-h-full object-contain rounded-xl border border-border shadow-2xl"
        />
      {/if}
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
