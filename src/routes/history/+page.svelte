<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "$lib/Icon.svelte";
  import ScrollArea from "$lib/ScrollArea.svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { mermaidBlocks } from "$lib/mermaid";

  /** Link trong câu trả lời đã lưu (VD nguồn trích dẫn) — mở bằng trình
   * duyệt hệ thống thay vì để WebView2 điều hướng nguyên cửa sổ app sang
   * trang đó (không có cách quay lại). */
  function handleAnswerLinkClick(e: MouseEvent) {
    const link = (e.target as HTMLElement).closest<HTMLAnchorElement>("a[href]");
    if (!link) return;
    e.preventDefault();
    openUrl(link.href).catch((err) => console.warn("[snip-ai] Không mở được link:", err));
  }

  // Cửa sổ Lịch sử — SINGLETON, riêng biệt hẳn với "Kết quả AI" (result-N).
  // Chỉ XEM + XOÁ trong bản này, không hỏi tiếp được ngay tại đây (media của
  // 1 phiên chỉ tồn tại trong AppState lúc cửa sổ "Kết quả AI" của phiên đó
  // còn mở — cửa sổ Lịch sử không có sẵn state đó để gọi ask_ai_* tiếp).

  interface HistoryTurn {
    role: string;
    content: string;
    displayLabel?: string;
  }
  interface ListEntry {
    id: string;
    kind: string;
    createdAt: number;
    model: string;
    preview: string;
    turnCount: number;
    mediaMissing: boolean;
  }
  interface ItemFull {
    id: string;
    kind: string;
    model: string;
    createdAt: number;
    turns: HistoryTurn[];
    mediaB64: string | null;
    mediaMime: string;
    mediaMissing: boolean;
  }

  let items = $state<ListEntry[]>([]);
  let loading = $state(true);
  let loadError = $state("");

  let selectedId = $state<string | null>(null);
  let detail = $state<ItemFull | null>(null);
  let detailLoading = $state(false);
  let detailError = $state("");

  /** Bấm 1 lần: chuyển nút "Xoá tất cả" sang trạng thái xác nhận trong vài
   * giây; bấm LẦN 2 trong lúc đó mới thực sự xoá. Đỡ phải dựng hẳn 1 modal
   * xác nhận cho 1 hành động không thể hoàn tác. */
  let confirmingClearAll = $state(false);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;

  /** Xoá theo id -> toast "Đã xoá — Hoàn tác" trong 5s, lệnh xoá THẬT trên
   * đĩa chỉ chạy khi toast tự tắt (không bấm Hoàn tác). Nhiều lượt xoá liên
   * tiếp thì xếp hàng độc lập (key = id), không đè lên nhau. */
  interface PendingDelete {
    entry: ListEntry;
    index: number;
    timer: ReturnType<typeof setTimeout>;
  }
  let pendingDeletes = $state<Map<string, PendingDelete>>(new Map());

  async function loadList() {
    loading = true;
    loadError = "";
    try {
      items = await invoke<ListEntry[]>("history_list");
    } catch (e) {
      loadError = String(e);
    } finally {
      loading = false;
    }
  }

  async function openDetail(id: string) {
    selectedId = id;
    detail = null;
    detailError = "";
    detailLoading = true;
    try {
      detail = await invoke<ItemFull>("history_get", { id });
    } catch (e) {
      detailError = String(e);
    } finally {
      detailLoading = false;
    }
  }

  function requestDelete(entry: ListEntry) {
    // Ẩn ngay khỏi danh sách (optimistic) — lệnh xoá thật trên đĩa bị TRÌ
    // HOÃN, chưa chạy cho tới khi hết giờ hoàn tác.
    const index = items.findIndex((it) => it.id === entry.id);
    items = items.filter((it) => it.id !== entry.id);
    if (selectedId === entry.id) {
      selectedId = null;
      detail = null;
    }

    const timer = setTimeout(() => {
      invoke("history_delete", { id: entry.id }).catch((e) => console.warn("[snip-ai] Xoá lịch sử thất bại:", e));
      pendingDeletes.delete(entry.id);
      pendingDeletes = new Map(pendingDeletes);
    }, 5000);

    pendingDeletes.set(entry.id, { entry, index, timer });
    pendingDeletes = new Map(pendingDeletes);
  }

  function undoDelete(id: string) {
    const pending = pendingDeletes.get(id);
    if (!pending) return;
    clearTimeout(pending.timer);
    pendingDeletes.delete(id);
    pendingDeletes = new Map(pendingDeletes);
    const restored = [...items];
    restored.splice(Math.min(pending.index, restored.length), 0, pending.entry);
    items = restored;
  }

  function handleClearAllClick() {
    if (!confirmingClearAll) {
      confirmingClearAll = true;
      confirmTimer = setTimeout(() => (confirmingClearAll = false), 3000);
      return;
    }
    clearTimeout(confirmTimer);
    confirmingClearAll = false;
    // Xoá tất cả là xoá THẬT NGAY — dành cho ca "dọn nhanh trước khi người
    // khác dùng máy chung", không cần (và không nên) có cửa sổ hoàn tác 5s
    // làm chậm việc dọn khẩn.
    invoke("history_clear_all").catch((e) => console.warn("[snip-ai] Xoá tất cả lịch sử thất bại:", e));
    for (const p of pendingDeletes.values()) clearTimeout(p.timer);
    pendingDeletes = new Map();
    items = [];
    selectedId = null;
    detail = null;
  }

  function formatRelative(ms: number): string {
    const diffSec = Math.max(0, (Date.now() - ms) / 1000);
    if (diffSec < 60) return "Vừa xong";
    if (diffSec < 3600) return `${Math.floor(diffSec / 60)} phút trước`;
    if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} giờ trước`;
    if (diffSec < 86400 * 7) return `${Math.floor(diffSec / 86400)} ngày trước`;
    const d = new Date(ms);
    return `${String(d.getDate()).padStart(2, "0")}/${String(d.getMonth() + 1).padStart(2, "0")}`;
  }

  onMount(() => {
    loadList();
  });
</script>

<div class="app-bg h-screen flex flex-col text-text overflow-hidden">
  <header class="glass shrink-0 px-4 py-3 flex items-center gap-2.5">
    <div
      class="w-7 h-7 rounded-lg flex items-center justify-center text-accent-text shrink-0"
      style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
    >
      <Icon name="clock" size={14} />
    </div>
    <h1 class="text-[14px] font-bold flex-1">Lịch sử</h1>
    {#if items.length > 0}
      <button
        onclick={handleClearAllClick}
        class="px-2.5 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5 transition-colors
          {confirmingClearAll
          ? 'bg-[color:var(--color-danger)] text-white'
          : 'btn-ghost text-[color:var(--color-danger)]'}"
      >
        <Icon name="trash" size={12} />
        {confirmingClearAll ? "Bấm lần nữa để xoá hết" : "Xoá tất cả"}
      </button>
    {/if}
  </header>

  <div class="flex-1 min-h-0 flex">
    <!-- Cột trái: danh sách -->
    <ScrollArea class="w-[280px] shrink-0 border-r border-border">
      {#if loading}
        <div class="flex flex-1 items-center justify-center text-text-muted">
          <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
        </div>
      {:else if loadError}
        <div class="p-4 text-[12px] text-[color:var(--color-danger)]">{loadError}</div>
      {:else if items.length === 0 && pendingDeletes.size === 0}
        <div class="flex flex-1 flex-col items-center justify-center gap-2 text-text-muted px-6 text-center">
          <Icon name="clock" size={22} class="opacity-40" />
          <p class="text-[12px] leading-relaxed">
            Chưa có gì trong lịch sử. Các lần snip/quay sau khi hỏi AI xong sẽ tự lưu ở đây, tự dọn sau 14 ngày.
          </p>
        </div>
      {:else}
        <div class="p-1.5 flex flex-col gap-1">
          {#each items as it (it.id)}
            <button
              onclick={() => openDetail(it.id)}
              class="text-left px-2.5 py-2 rounded-xl transition-colors flex items-start gap-2 {selectedId === it.id
                ? 'bg-[var(--surface-hover)]'
                : 'hover:bg-[var(--surface-hover)]'}"
            >
              <div
                class="shrink-0 w-7 h-7 rounded-lg flex items-center justify-center text-accent-text mt-0.5"
                style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
              >
                <Icon name={it.kind === "video" ? "video" : "image"} size={13} />
              </div>
              <div class="flex-1 min-w-0">
                <div class="text-[12px] font-medium truncate leading-tight">{it.preview}</div>
                <div class="text-[10.5px] text-text-muted mt-0.5 flex items-center gap-1">
                  {formatRelative(it.createdAt)}
                  {#if it.mediaMissing}
                    <span class="text-[color:var(--color-danger)]">· mất ảnh/video gốc</span>
                  {/if}
                </div>
              </div>
              <span
                role="button"
                tabindex="0"
                onclick={(e) => {
                  e.stopPropagation();
                  requestDelete(it);
                }}
                onkeydown={(e) => e.key === "Enter" && requestDelete(it)}
                class="shrink-0 p-1 rounded-md text-text-muted hover:text-[color:var(--color-danger)] hover:bg-[color:var(--color-danger)]/10 transition-colors"
                title="Xoá"
              >
                <Icon name="x" size={12} />
              </span>
            </button>
          {/each}
        </div>
      {/if}
    </ScrollArea>

    <!-- Cột phải: chi tiết -->
    <ScrollArea class="flex-1 min-w-0" contentClass="p-4">
      {#if !selectedId}
        <div class="flex flex-1 items-center justify-center text-text-muted text-[12px]">
          Chọn 1 mục bên trái để xem lại
        </div>
      {:else if detailLoading}
        <div class="flex flex-1 items-center justify-center text-text-muted">
          <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
        </div>
      {:else if detailError}
        <div class="text-[12px] text-[color:var(--color-danger)]">{detailError}</div>
      {:else if detail}
        <div class="max-w-2xl mx-auto flex flex-col gap-4">
          {#if detail.mediaMissing}
            <div
              class="rounded-xl border border-dashed border-[color:var(--color-danger)]/40 bg-[color:var(--color-danger)]/5 px-4 py-6 flex flex-col items-center gap-2 text-center"
            >
              <Icon name="alert" size={18} class="text-[color:var(--color-danger)]" />
              <p class="text-[12px] text-text-muted leading-relaxed">
                Ảnh/video gốc của mục này đã bị xoá (có thể do xoá thủ công ngoài ứng dụng). Nội dung hội thoại bên
                dưới vẫn còn.
              </p>
            </div>
          {:else if detail.mediaB64}
            {#if detail.kind === "video"}
              <!-- svelte-ignore a11y_media_has_caption -->
              <!-- Video tự quay bằng app này KHÔNG có track âm thanh (tắt hẳn
              ở record.rs) nên không có gì để phụ đề. -->
              <video
                src={`data:${detail.mediaMime};base64,${detail.mediaB64}`}
                controls
                class="w-full rounded-xl border border-border shadow-lg"
              ></video>
            {:else}
              <img
                src={`data:${detail.mediaMime};base64,${detail.mediaB64}`}
                alt="Ảnh đã chụp"
                class="w-full rounded-xl border border-border shadow-lg object-contain"
              />
            {/if}
          {/if}

          <div class="flex flex-col gap-3">
            {#each detail.turns as turn, i (i)}
              {#if turn.role === "user"}
                <div class="flex justify-end">
                  <div
                    class="max-w-[86%] rounded-2xl rounded-br-md px-3.5 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap text-accent-text font-medium"
                    style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
                  >
                    {turn.displayLabel ?? turn.content}
                  </div>
                </div>
              {:else}
                <div class="flex items-start gap-2.5">
                  <div
                    class="shrink-0 w-6 h-6 mt-0.5 rounded-lg flex items-center justify-center text-accent-text"
                    style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
                  >
                    <Icon name="sparkles" size={12} strokeWidth={2.3} />
                  </div>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- Chặn link tự điều hướng nguyên cửa sổ app sang trang
                  web (WebView2 mặc định làm vậy) — mở bằng trình duyệt hệ
                  thống thay vào đó. Cùng lý do/cách làm với result/+page.svelte. -->
                  <!-- min-w-0: ĐÈ mặc định min-width:auto của flex item — nội
                  dung không ngắt được (VD công thức KaTeX dài) sẽ ép cả div
                  này rộng hơn max-w-[88%] nếu thiếu dòng này, xem giải thích
                  đầy đủ ở result/+page.svelte (cùng bug, cùng cách sửa). -->
                  <div
                    class="markdown-body card max-w-[88%] min-w-0 rounded-2xl rounded-tl-md px-3.5 py-2.5 text-[12.5px]"
                    onclick={handleAnswerLinkClick}
                    use:mermaidBlocks={turn.content}
                  >
                    {@html renderMarkdown(turn.content)}
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      {/if}
    </ScrollArea>
  </div>

  <!-- Toast "Đã xoá — Hoàn tác", 1 cái cho mỗi lượt xoá đang chờ -->
  {#if pendingDeletes.size > 0}
    <div class="fixed bottom-4 left-1/2 -translate-x-1/2 flex flex-col gap-2 z-30">
      {#each [...pendingDeletes.values()] as p (p.entry.id)}
        <div
          class="card px-3.5 py-2 rounded-xl flex items-center gap-3 shadow-lg"
          transition:fade={{ duration: 140 }}
        >
          <span class="text-[12px]">Đã xoá "{p.entry.preview}"</span>
          <button onclick={() => undoDelete(p.entry.id)} class="text-[12px] font-semibold text-accent shrink-0">
            Hoàn tác
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>
