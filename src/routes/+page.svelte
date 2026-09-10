<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";
  import { captureCombo, formatKeyLabel, getHotkey, setHotkey } from "$lib/hotkey";

  let toast = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  // ── Đăng nhập Google — DUY NHẤT cách dùng AI trong app, không còn mục
  // "nhà cung cấp AI nâng cao" / tự nhập API key nữa (quá phức tạp với đối
  // tượng chính: học sinh/sinh viên, văn phòng). Widget hiện ở góc trên bên
  // phải như các app phổ biến khác (Google Docs, VS Code...), có avatar thật
  // của tài khoản Google đang đăng nhập. ─────────────────────────────────
  interface LoginStatus {
    email: string;
    picture?: string;
  }
  let loginStatus = $state<LoginStatus | null>(null);
  let loginBusy = $state(false);
  let loginError = $state("");
  let showAccountMenu = $state(false);

  async function refreshLoginStatus() {
    try {
      loginStatus = await invoke<LoginStatus | null>("get_login_status");
    } catch {
      loginStatus = null;
    }
  }

  async function handleGoogleLogin() {
    loginBusy = true;
    loginError = "";
    try {
      loginStatus = await invoke<LoginStatus>("start_google_login");
      flash("ok", `Đã đăng nhập ${loginStatus.email}`);
    } catch (e) {
      loginError = String(e);
    } finally {
      loginBusy = false;
    }
  }

  async function handleLogout() {
    loginBusy = true;
    showAccountMenu = false;
    try {
      await invoke("logout");
      loginStatus = null;
      flash("ok", "Đã đăng xuất");
    } catch (e) {
      flash("err", String(e));
    } finally {
      loginBusy = false;
    }
  }

  // ── Phím tắt ──────────────────────────────────────────────────────────
  let hotkeyParts = $state<string[]>(["Ctrl", "PrintScreen"]);
  let recordingHotkey = $state(false);
  let hotkeyBusy = $state(false);
  let hotkeyError = $state("");

  async function loadHotkey() {
    try {
      const accel = await getHotkey();
      hotkeyParts = accel.split("+").map(formatKeyLabel);
    } catch (e) {
      hotkeyError = String(e);
    }
  }

  function onHotkeyKeydown(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      stopRecording();
      return;
    }
    if (e.repeat) return;

    const combo = captureCombo(e);
    if (!combo) return; // mới bấm modifier, hoặc phím chưa nhận diện được -> chờ tiếp

    stopRecording();
    applyHotkey(combo.accelerator, combo.parts);
  }

  async function applyHotkey(accelerator: string, parts: string[]) {
    hotkeyBusy = true;
    hotkeyError = "";
    try {
      const confirmed = await setHotkey(accelerator);
      hotkeyParts = confirmed.split("+").map(formatKeyLabel);
      flash("ok", `Đã đổi phím tắt: ${hotkeyParts.join(" + ")}`);
    } catch (e) {
      hotkeyError = String(e);
      hotkeyParts = parts; // vẫn hiện tổ hợp vừa bấm để người dùng biết mình bấm gì, kèm lỗi bên dưới
    } finally {
      hotkeyBusy = false;
    }
  }

  function startRecording() {
    recordingHotkey = true;
    hotkeyError = "";
    window.addEventListener("keydown", onHotkeyKeydown, { capture: true });
  }

  function stopRecording() {
    recordingHotkey = false;
    window.removeEventListener("keydown", onHotkeyKeydown, { capture: true });
  }

  function flash(kind: "ok" | "err", text: string) {
    toast = { kind, text };
    setTimeout(() => (toast = null), 2600);
  }

  onMount(() => {
    // onMount không cho callback async trả về hàm dọn dẹp (Promise != function),
    // nên tách phần load dữ liệu ra 1 hàm async gọi rời, còn onMount chỉ trả
    // thẳng cleanup function.
    loadHotkey();
    refreshLoginStatus();
    return () => stopRecording(); // dọn listener nếu rời trang giữa lúc đang ghi phím
  });
</script>

<main class="app-bg min-h-screen text-text flex flex-col">
  <!-- Top bar -->
  <header class="glass sticky top-0 z-10 px-5 py-3.5 flex items-center gap-3">
    <div
      class="w-8 h-8 rounded-xl flex items-center justify-center text-accent-text shrink-0"
      style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
    >
      <Icon name="scan" size={17} strokeWidth={2.4} />
    </div>
    <div class="flex-1 min-w-0">
      <h1 class="text-[15px] font-bold leading-tight">Snap AI</h1>
      <p class="text-[11px] text-text-muted leading-tight">Chụp màn hình · Hỏi AI</p>
    </div>

    <!-- Tài khoản — góc trên bên phải, avatar thật nếu có ảnh Google -->
    <div class="relative shrink-0">
      {#if loginStatus}
        <button
          onclick={() => (showAccountMenu = !showAccountMenu)}
          class="flex items-center gap-1.5 rounded-full pl-1 pr-1.5 py-1 hover:bg-[var(--surface-hover)] transition-colors"
        >
          {#if loginStatus.picture}
            <img
              src={loginStatus.picture}
              alt=""
              referrerpolicy="no-referrer"
              class="w-7 h-7 rounded-full object-cover border border-border"
            />
          {:else}
            <div
              class="w-7 h-7 rounded-full flex items-center justify-center text-[12px] font-bold text-accent-text shrink-0"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              {loginStatus.email.charAt(0).toUpperCase()}
            </div>
          {/if}
          <Icon name="chevronDown" size={12} class="text-text-muted" />
        </button>

        {#if showAccountMenu}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <button
            class="fixed inset-0 z-10 cursor-default"
            style="background: transparent;"
            onclick={() => (showAccountMenu = false)}
            aria-label="Đóng menu tài khoản"
          ></button>
          <div class="absolute right-0 top-full mt-2 w-56 card p-1.5 z-20" transition:fade={{ duration: 120 }}>
            <div class="px-2.5 py-2">
              <div class="text-[12px] font-semibold truncate">{loginStatus.email}</div>
              <div class="text-[10.5px] text-text-muted">Đã đăng nhập bằng Google</div>
            </div>
            <div class="h-px bg-border my-0.5"></div>
            <button
              onclick={handleLogout}
              disabled={loginBusy}
              class="w-full text-left px-2.5 py-1.5 rounded-lg text-[12.5px] font-medium text-[color:var(--color-danger)] hover:bg-[var(--surface-hover)] transition-colors disabled:opacity-50"
            >
              Đăng xuất
            </button>
          </div>
        {/if}
      {:else}
        <button
          onclick={handleGoogleLogin}
          disabled={loginBusy}
          class="btn-accent px-3.5 py-1.5 rounded-lg text-[12.5px] font-semibold flex items-center gap-1.5 disabled:opacity-60"
        >
          {#if loginBusy}
            Đang mở…
          {:else}
            Đăng nhập
          {/if}
        </button>
      {/if}
    </div>
  </header>

  {#if !loginStatus}
    <!-- Chưa đăng nhập: lời mời gọi hành động rõ ràng — bắt buộc phải đăng
    nhập mới dùng được AI (không còn đường lùi "tự nhập API key" nữa). Giữ
    khối phím tắt gọn bên dưới, không cần chờ đăng nhập mới đổi được. -->
    <div class="flex-1 overflow-y-auto p-5 flex flex-col gap-4">
      <div class="card p-5 flex flex-col items-center text-center gap-3" transition:fade={{ duration: 160 }}>
        <div
          class="w-12 h-12 rounded-2xl flex items-center justify-center text-accent-text"
          style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
        >
          <Icon name="sparkles" size={22} />
        </div>
        <div>
          <h2 class="text-[14px] font-bold">Đăng nhập để bắt đầu</h2>
          <p class="text-[12px] text-text-muted leading-relaxed mt-1 max-w-[280px]">
            Đăng nhập bằng tài khoản Google — dùng AI ngay, không cần tự tạo hay nhập bất kỳ API key nào.
          </p>
        </div>
        <button
          onclick={handleGoogleLogin}
          disabled={loginBusy}
          class="btn-accent px-5 py-2.5 rounded-lg text-[13.5px] font-semibold flex items-center gap-2 disabled:opacity-60"
        >
          {#if loginBusy}
            Đang mở trình duyệt…
          {:else}
            <Icon name="sparkles" size={15} /> Đăng nhập bằng Google
          {/if}
        </button>
        {#if loginError}
          <p class="text-[11.5px] text-[color:var(--color-danger)] selectable leading-relaxed">{loginError}</p>
        {/if}
      </div>

      <button
        onclick={() => (recordingHotkey ? stopRecording() : startRecording())}
        disabled={hotkeyBusy}
        class="btn-ghost self-center px-3 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5"
      >
        {#if recordingHotkey}
          <span class="text-accent animate-pulse">Nhấn tổ hợp phím mới…</span>
        {:else}
          <Icon name="keyboard" size={12} />
          {hotkeyParts.join(" + ")}
          <Icon name="edit" size={11} class="text-text-muted" />
        {/if}
      </button>
      {#if hotkeyError}
        <p class="text-[11px] text-[color:var(--color-danger)] selectable leading-relaxed text-center" transition:fade={{ duration: 140 }}>
          {hotkeyError}
        </p>
      {/if}
    </div>
  {:else}
    <!-- Đã đăng nhập: giống màn hình chờ của Snipping Tool — chỉ 1 dòng nhắc
    phím tắt ở giữa cửa sổ, không còn UI thừa nào khác (không cần đăng nhập
    lại/chọn hành động gì thêm, mọi thứ đã sẵn sàng dùng ngay). -->
    <div class="flex-1 flex flex-col items-center justify-center gap-3 px-6 text-center" transition:fade={{ duration: 160 }}>
      <p class="text-[13.5px] text-text-muted leading-relaxed">
        {#if recordingHotkey}
          <span class="text-accent font-medium animate-pulse">Nhấn tổ hợp phím mới…</span>
        {:else}
          Nhấn
          {#each hotkeyParts as part, i (i)}
            {#if i > 0}<span class="mx-1 text-text-muted">+</span>{/if}
            <kbd class="px-1.5 py-0.5 rounded-md bg-bg-elevated border border-border text-[12px] text-text font-mono align-middle"
              >{part}</kbd
            >
          {/each}
          để bắt đầu snip
        {/if}
      </p>
      <button
        onclick={() => (recordingHotkey ? stopRecording() : startRecording())}
        disabled={hotkeyBusy}
        class="btn-ghost px-3 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5"
      >
        {#if recordingHotkey}
          <Icon name="x" size={12} /> Huỷ
        {:else}
          <Icon name="edit" size={12} /> Đổi phím tắt
        {/if}
      </button>
      {#if hotkeyError}
        <p class="text-[11px] text-[color:var(--color-danger)] selectable leading-relaxed" transition:fade={{ duration: 140 }}>
          {hotkeyError}
        </p>
      {/if}
    </div>
  {/if}

  <!-- Toast -->
  {#if toast}
    <div
      class="fixed bottom-4 left-1/2 -translate-x-1/2 z-20 px-4 py-2.5 rounded-xl text-[12.5px] font-medium flex items-center gap-2 shadow-lg glass border {toast.kind ===
      'ok'
        ? 'border-[color:var(--color-accent)]/40 text-accent'
        : 'border-[color:var(--color-danger)]/40 text-[color:var(--color-danger)]'}"
      transition:fade={{ duration: 160 }}
    >
      <Icon name={toast.kind === "ok" ? "check" : "alert"} size={14} />
      <span class="selectable">{toast.text}</span>
    </div>
  {/if}
</main>
