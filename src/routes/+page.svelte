<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";
  import ScrollArea from "$lib/ScrollArea.svelte";
  import {
    captureCombo,
    formatKeyLabel,
    getHotkey,
    getRecordHotkey,
    setHotkey,
    setRecordHotkey,
  } from "$lib/hotkey";
  import { loadTheme, setTheme, type ThemeMode } from "$lib/theme";

  let toast = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  // ── Khảo sát mức độ hài lòng — lịch hiện "thông minh" quyết định HOÀN
  // TOÀN ở phía Rust (survey.rs): chỉ hỏi sau khi đã dùng app đủ nhiều, đã
  // khảo sát rồi thì không hỏi lại, bỏ qua thì chờ 1 khoảng mới hỏi lại. Cửa
  // sổ này (Settings/"main") chỉ ẨN chứ không đóng khi bấm X (xem lib.rs) —
  // kiểm tra lại mỗi lần cửa sổ được HIỆN LẠI (sự kiện focus), không chỉ lúc
  // mount lần đầu, vì cửa sổ có thể ở ẩn rất lâu trong lúc người dùng snip/
  // chat ở các cửa sổ khác. ─────────────────────────────────────────────
  type SurveyRating = "unhappy" | "happy" | "very_happy";
  let showSurveyModal = $state(false);
  let surveyRating = $state<SurveyRating | null>(null);
  let surveyComment = $state("");
  let surveySubmitting = $state(false);
  let surveyError = $state("");

  async function checkSurveyEligibility() {
    // Đang mở sẵn modal khác (menu tài khoản không tính, nhưng tránh phiền
    // nếu lỡ đang bận thao tác gì) — bỏ qua lần check này, lần focus SAU vẫn
    // sẽ thử lại (không mất hẳn cơ hội hỏi, chỉ lùi lại).
    if (showSurveyModal) return;
    try {
      const status = await invoke<{ eligible: boolean }>("survey_status");
      if (status.eligible) showSurveyModal = true;
    } catch (e) {
      console.warn("[snip-ai] Không kiểm tra được lịch khảo sát:", e);
    }
  }

  function resetSurveyForm() {
    surveyRating = null;
    surveyComment = "";
    surveyError = "";
  }

  async function handleSurveySubmit() {
    if (!surveyRating || surveySubmitting) return;
    surveySubmitting = true;
    surveyError = "";
    try {
      await invoke("submit_survey", { rating: surveyRating, comment: surveyComment.trim() });
      showSurveyModal = false;
      resetSurveyForm();
      flash("ok", "Cảm ơn bạn đã góp ý!");
    } catch (e) {
      surveyError = String(e);
    } finally {
      surveySubmitting = false;
    }
  }

  /** Bấm "Để sau"/đóng modal mà KHÔNG gửi — vẫn ghi nhận để áp dụng cooldown
   * (xem survey.rs::dismiss_survey), không phải huỷ hẳn tính năng khảo sát. */
  async function handleSurveyDismiss() {
    showSurveyModal = false;
    resetSurveyForm();
    try {
      await invoke("dismiss_survey");
    } catch (e) {
      console.warn("[snip-ai] Không lưu được lượt bỏ qua khảo sát:", e);
    }
  }

  // ── Chế độ Ảnh/Video — 2 nút kiểu Snipping Tool, bấm chuyển qua lại xem
  // phím tắt nào (chỉ đổi PHẦN HIỂN THỊ trong Cài đặt, không tắt phím tắt còn
  // lại — cả 2 phím tắt vẫn hoạt động song song lúc dùng thật). ─────────────
  type CaptureMode = "snip" | "record";
  let captureMode = $state<CaptureMode>("snip");

  // ── Chế độ sáng/tối/hệ thống ─────────────────────────────────────────
  let themeMode = $state<ThemeMode>("system");
  const THEME_OPTIONS: { mode: ThemeMode; icon: string; title: string }[] = [
    { mode: "light", icon: "sun", title: "Sáng" },
    { mode: "system", icon: "monitor", title: "Theo hệ thống" },
    { mode: "dark", icon: "moon", title: "Tối" },
  ];
  const currentThemeOption = $derived(THEME_OPTIONS.find((o) => o.mode === themeMode) ?? THEME_OPTIONS[1]);

  // ── Nút "+ New" — bấm trực tiếp để snip/quay (thay vì phải nhớ bấm phím
  // tắt). Chạy đúng hành động theo `captureMode` đang chọn ở toggle header. ──
  let newActionBusy = $state(false);
  let newActionError = $state("");
  async function handleNewAction() {
    newActionBusy = true;
    newActionError = "";
    try {
      await invoke(captureMode === "snip" ? "trigger_capture" : "trigger_recording_from_ui");
    } catch (e) {
      newActionError = String(e);
    } finally {
      newActionBusy = false;
    }
  }

  /** Bấm 1 nút để chuyển vòng qua từng chế độ theo đúng thứ tự trong
   * THEME_OPTIONS, quay lại đầu khi hết — không cần hiện cả 3 lựa chọn cùng lúc. */
  function cycleTheme() {
    const i = THEME_OPTIONS.findIndex((o) => o.mode === themeMode);
    const next = THEME_OPTIONS[(i + 1) % THEME_OPTIONS.length];
    themeMode = next.mode;
    setTheme(next.mode);
  }

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
  let capturingHotkey = $state(false);
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
      stopHotkeyCapture();
      return;
    }
    if (e.repeat) return;

    const combo = captureCombo(e);
    if (!combo) return; // mới bấm modifier, hoặc phím chưa nhận diện được -> chờ tiếp

    stopHotkeyCapture();
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

  function startHotkeyCapture() {
    capturingHotkey = true;
    hotkeyError = "";
    window.addEventListener("keydown", onHotkeyKeydown, { capture: true });
  }

  function stopHotkeyCapture() {
    capturingHotkey = false;
    window.removeEventListener("keydown", onHotkeyKeydown, { capture: true });
  }

  // ── Phím tắt QUAY VIDEO — độc lập với phím snip ảnh ở trên ────────────
  let videoHotkeyParts = $state<string[]>(["Ctrl", "Shift", "PrintScreen"]);
  let capturingVideoHotkey = $state(false);
  let videoHotkeyBusy = $state(false);
  let videoHotkeyError = $state("");

  async function loadVideoHotkey() {
    try {
      const accel = await getRecordHotkey();
      videoHotkeyParts = accel.split("+").map(formatKeyLabel);
    } catch (e) {
      videoHotkeyError = String(e);
    }
  }

  function onVideoHotkeyKeydown(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      stopVideoHotkeyCapture();
      return;
    }
    if (e.repeat) return;

    const combo = captureCombo(e);
    if (!combo) return;

    stopVideoHotkeyCapture();
    applyVideoHotkey(combo.accelerator, combo.parts);
  }

  async function applyVideoHotkey(accelerator: string, parts: string[]) {
    videoHotkeyBusy = true;
    videoHotkeyError = "";
    try {
      const confirmed = await setRecordHotkey(accelerator);
      videoHotkeyParts = confirmed.split("+").map(formatKeyLabel);
      flash("ok", `Đã đổi phím tắt quay video: ${videoHotkeyParts.join(" + ")}`);
    } catch (e) {
      videoHotkeyError = String(e);
      videoHotkeyParts = parts;
    } finally {
      videoHotkeyBusy = false;
    }
  }

  function startVideoHotkeyCapture() {
    capturingVideoHotkey = true;
    videoHotkeyError = "";
    window.addEventListener("keydown", onVideoHotkeyKeydown, { capture: true });
  }

  function stopVideoHotkeyCapture() {
    capturingVideoHotkey = false;
    window.removeEventListener("keydown", onVideoHotkeyKeydown, { capture: true });
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
    loadVideoHotkey();
    refreshLoginStatus();
    themeMode = loadTheme();
    // Trễ 1 chút lúc mới mở app — không tranh giành sự chú ý với các bước
    // đầu (đăng nhập...) diễn ra ngay khi cửa sổ vừa hiện.
    setTimeout(checkSurveyEligibility, 1500);
    // Cửa sổ này chỉ ẨN (không đóng) khi bấm X — "focus" của trình duyệt bắn
    // lại mỗi lần Rust show() + set_focus() nó (qua tray/hotkey), đúng lúc
    // cần kiểm tra lại vì có thể đã đủ điều kiện từ lúc ẩn tới giờ.
    window.addEventListener("focus", checkSurveyEligibility);
    return () => {
      stopHotkeyCapture();
      stopVideoHotkeyCapture();
      window.removeEventListener("focus", checkSurveyEligibility);
    }; // dọn listener nếu rời trang giữa lúc đang ghi phím
  });
</script>

<main class="app-bg min-h-screen text-text flex flex-col">
  <!-- Top bar -->
  <!-- z-30: PHẢI cao hơn thanh toggle Ảnh/Video bên dưới (z-20) — header
  TỰ TẠO 1 stacking context riêng (position:sticky + z-index), nên menu tài
  khoản dù nằm TRONG header có z-index cao đến đâu cũng bị "nhốt" trong
  stacking context z-10 của header, không thể vượt qua z-20 của toggle nằm
  NGOÀI header được (bài học rút ra: z-index chỉ so sánh được giữa các phần
  tử CÙNG 1 stacking context — nâng z-index của con bên trong không có tác
  dụng nếu chính cha đã thấp hơn phần tử cần vượt qua). -->
  <header class="glass sticky top-0 z-30 px-4 py-3.5 flex items-center gap-2">
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

    <!-- Mở cửa sổ Lịch sử — singleton (Rust tự show/focus lại cửa sổ cũ nếu
    đã mở, không tạo chồng), xem commands.rs::open_history_window. -->
    <button
      onclick={() => invoke("open_history_window")}
      title="Lịch sử"
      aria-label="Mở lịch sử"
      class="btn-ghost w-7 h-7 rounded-full flex items-center justify-center shrink-0"
    >
      <Icon name="clock" size={14} />
    </button>

    <!-- Công tắc sáng/tối/hệ thống — 1 nút bấm để chuyển vòng qua từng chế
    độ (Sáng -> Hệ thống -> Tối -> Sáng...), thay vì hiện cả 3 lựa chọn dàn
    trải cùng lúc. -->
    <button
      onclick={cycleTheme}
      title={`Giao diện: ${currentThemeOption.title} (bấm để đổi)`}
      aria-label="Đổi chế độ sáng/tối"
      class="icon-btn-accent w-7 h-7 rounded-full flex items-center justify-center shrink-0 text-accent-text"
      style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
    >
      <Icon name={currentThemeOption.icon} size={13} strokeWidth={2.2} />
    </button>

    <!-- Tài khoản — góc trên bên phải, avatar thật nếu có ảnh Google -->
    <div class="relative shrink-0">
      {#if loginStatus}
        <button
          onclick={() => (showAccountMenu = !showAccountMenu)}
          class="group flex items-center gap-1.5 rounded-full pl-1 pr-1.5 py-1 hover:bg-[var(--surface-hover)] transition-colors"
        >
          {#if loginStatus.picture}
            <img
              src={loginStatus.picture}
              alt=""
              referrerpolicy="no-referrer"
              class="w-7 h-7 rounded-full object-cover border border-border transition-transform duration-150 group-hover:scale-110"
            />
          {:else}
            <div
              class="w-7 h-7 rounded-full flex items-center justify-center text-[12px] font-bold text-accent-text shrink-0 transition-transform duration-150 group-hover:scale-110"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              {loginStatus.email.charAt(0).toUpperCase()}
            </div>
          {/if}
          <Icon
            name="chevronDown"
            size={12}
            class="text-text-muted transition-transform duration-150 {showAccountMenu ? 'rotate-180' : ''}"
          />
        </button>

        {#if showAccountMenu}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <button
            class="fixed inset-0 z-30 cursor-default"
            style="background: transparent;"
            onclick={() => (showAccountMenu = false)}
            aria-label="Đóng menu tài khoản"
          ></button>
          <!-- z-40: PHẢI cao hơn thanh toggle Ảnh/Video bên dưới header
          (cũng z-20) — cùng z-index thì phần tử đứng SAU trong DOM (toggle)
          vẽ đè lên trên, che mất menu này dù về mặt UX nó phải là popover nổi
          trên cùng (bug thực tế đã gặp: mở menu tài khoản bị thanh toggle
          che mất nửa dưới). -->
          <div class="absolute right-0 top-full mt-2 w-56 card p-1.5 z-40" transition:fade={{ duration: 120 }}>
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

  <!-- Toggle Ảnh/Video kiểu Snipping Tool — mép trên sát ngay đường viền dưới
  header (không đè lên), dùng CHUNG cho cả 2 trạng thái đăng nhập/chưa, chỉ
  đổi PHẦN HIỂN THỊ phím tắt bên dưới — không tắt phím tắt còn lại (cả 2 vẫn
  hoạt động song song lúc dùng thật). -->
  <div class="relative z-20 flex justify-center">
    <div
      class="flex items-center rounded-full p-0.5 shadow-md"
      style="background: var(--color-card); border: 1px solid var(--color-border);"
    >
      {#each [{ mode: "snip", icon: "camera" }, { mode: "record", icon: "video" }] as m (m.mode)}
        <button
          onclick={() => (captureMode = m.mode as CaptureMode)}
          class="relative w-9 h-8 rounded-full flex items-center justify-center transition-colors {captureMode ===
          m.mode
            ? 'text-accent'
            : 'text-text-muted hover:text-text'}"
        >
          <Icon name={m.icon} size={15} />
          {#if captureMode === m.mode}
            <span
              class="absolute left-2.5 right-2.5 -bottom-0.5 h-0.5 rounded-full"
              style="background: var(--color-accent);"
              transition:fade={{ duration: 120 }}
            ></span>
          {/if}
        </button>
      {/each}
    </div>
  </div>

  {#if !loginStatus}
    <!-- Chưa đăng nhập: lời mời gọi hành động rõ ràng — bắt buộc phải đăng
    nhập mới dùng được AI (không còn đường lùi "tự nhập API key" nữa). Giữ
    khối phím tắt gọn bên dưới, không cần chờ đăng nhập mới đổi được. -->
    <ScrollArea class="flex-1" contentClass="p-5 flex flex-col gap-4">
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
            Đăng nhập bằng tài khoản Google để dùng AI ngay!
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

      {#if captureMode === "snip"}
        <button
          onclick={() => (capturingHotkey ? stopHotkeyCapture() : startHotkeyCapture())}
          disabled={hotkeyBusy}
          class="btn-ghost self-center px-3 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5"
        >
          {#if capturingHotkey}
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
      {:else}
        <button
          onclick={() => (capturingVideoHotkey ? stopVideoHotkeyCapture() : startVideoHotkeyCapture())}
          disabled={videoHotkeyBusy}
          class="btn-ghost self-center px-3 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5"
        >
          {#if capturingVideoHotkey}
            <span class="text-accent animate-pulse">Nhấn tổ hợp phím mới…</span>
          {:else}
            <Icon name="video" size={12} />
            {videoHotkeyParts.join(" + ")}
            <Icon name="edit" size={11} class="text-text-muted" />
          {/if}
        </button>
        {#if videoHotkeyError}
          <p class="text-[11px] text-[color:var(--color-danger)] selectable leading-relaxed text-center" transition:fade={{ duration: 140 }}>
            {videoHotkeyError}
          </p>
        {/if}
      {/if}
    </ScrollArea>
  {:else}
    <!-- Đã đăng nhập: giống màn hình chờ của Snipping Tool — nút "+ New" to,
    dễ bấm ở giữa (cho người không nhớ/không quen phím tắt), kèm 1 dòng chú
    thích phím tắt tương ứng ngay bên dưới. Toggle Ảnh/Video nằm ở header
    (gắn trên đường viền), không phải ở đây. -->
    <div class="flex-1 flex flex-col items-center justify-center gap-3 px-6 text-center" transition:fade={{ duration: 160 }}>
      <button
        onclick={handleNewAction}
        disabled={newActionBusy}
        class="btn-accent btn-new px-7 py-3 rounded-full text-[14.5px] font-semibold tracking-[0.01em] flex items-center gap-2.5 disabled:opacity-60"
      >
        {#if newActionBusy}
          <Icon name="loader" size={16} class="animate-spin" />
          Đang mở…
        {:else}
          <!-- Dấu cộng đặt trong đĩa tròn mờ: vừa ghim nó thẳng hàng với chữ
          (icon SVG cân giữa sẵn, khác ký tự "+" lệch baseline trước đây), vừa
          tạo điểm nhấn thị giác cho nút hành động chính. -->
          <span class="grid place-items-center w-5 h-5 rounded-full bg-[color:var(--color-accent-text)]/15">
            <Icon name="plus" size={13} strokeWidth={2.75} />
          </span>
          New
        {/if}
      </button>
      {#if newActionError}
        <p class="text-[11px] text-[color:var(--color-danger)] selectable leading-relaxed">{newActionError}</p>
      {/if}

      {#if captureMode === "snip"}
        <p class="text-[13.5px] text-text-muted leading-relaxed">
          {#if capturingHotkey}
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
          onclick={() => (capturingHotkey ? stopHotkeyCapture() : startHotkeyCapture())}
          disabled={hotkeyBusy}
          class="btn-ghost px-3 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5"
        >
          {#if capturingHotkey}
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
      {:else}
        <p class="text-[13.5px] text-text-muted leading-relaxed">
          {#if capturingVideoHotkey}
            <span class="text-accent font-medium animate-pulse">Nhấn tổ hợp phím mới…</span>
          {:else}
            Nhấn
            {#each videoHotkeyParts as part, i (i)}
              {#if i > 0}<span class="mx-1 text-text-muted">+</span>{/if}
              <kbd class="px-1.5 py-0.5 rounded-md bg-bg-elevated border border-border text-[12px] text-text font-mono align-middle"
                >{part}</kbd
              >
            {/each}
            để quay video (tối đa 30s)
          {/if}
        </p>
        <button
          onclick={() => (capturingVideoHotkey ? stopVideoHotkeyCapture() : startVideoHotkeyCapture())}
          disabled={videoHotkeyBusy}
          class="btn-ghost px-3 py-1.5 rounded-lg text-[11.5px] font-medium flex items-center gap-1.5"
        >
          {#if capturingVideoHotkey}
            <Icon name="x" size={12} /> Huỷ
          {:else}
            <Icon name="edit" size={12} /> Đổi phím tắt
          {/if}
        </button>
        {#if videoHotkeyError}
          <p class="text-[11px] text-[color:var(--color-danger)] selectable leading-relaxed" transition:fade={{ duration: 140 }}>
            {videoHotkeyError}
          </p>
        {/if}
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

  <!-- Khảo sát mức độ hài lòng — xem checkSurveyEligibility. "Để sau" (nút X)
  KHÔNG coi là huỷ hẳn, chỉ lùi lịch hỏi lại (dismiss_survey ở Rust). -->
  {#if showSurveyModal}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-5"
      role="presentation"
      onclick={handleSurveyDismiss}
      transition:fade={{ duration: 160 }}
    >
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="card w-full max-w-[340px] p-5 flex flex-col gap-4 relative" onclick={(e) => e.stopPropagation()}>
        <button
          onclick={handleSurveyDismiss}
          class="absolute top-3 right-3 btn-ghost p-1.5 rounded-lg"
          title="Để sau"
        >
          <Icon name="x" size={14} />
        </button>

        <div class="text-center">
          <h2 class="text-[14.5px] font-bold">Bạn thấy Snap AI thế nào?</h2>
          <p class="text-[11.5px] text-text-muted mt-1">Vài giây góp ý giúp app tốt hơn</p>
        </div>

        <div class="flex justify-center gap-3">
          {#each [{ v: "unhappy", icon: "faceFrown", label: "Không hài lòng" }, { v: "happy", icon: "faceMeh", label: "Hài lòng" }, { v: "very_happy", icon: "faceSmileBig", label: "Rất hài lòng" }] as opt (opt.v)}
            <button
              type="button"
              onclick={() => (surveyRating = opt.v as SurveyRating)}
              title={opt.label}
              class="flex flex-col items-center gap-1 p-2 rounded-xl border transition-colors {surveyRating === opt.v
                ? 'border-accent text-accent'
                : 'border-border text-text-muted hover:text-text hover:border-[color:var(--color-text)]/30'}"
              style={surveyRating === opt.v ? "background: color-mix(in srgb, var(--color-accent) 12%, transparent);" : ""}
            >
              <Icon name={opt.icon} size={26} strokeWidth={1.7} />
              <span class="text-[9.5px] font-medium leading-none">{opt.label}</span>
            </button>
          {/each}
        </div>

        <textarea
          bind:value={surveyComment}
          rows="3"
          placeholder="Bạn muốn góp ý gì thêm về app không? (không bắt buộc)"
          class="field selectable resize-none text-[12px]"
        ></textarea>

        {#if surveyError}
          <p class="text-[11px] text-[color:var(--color-danger)] selectable leading-relaxed">{surveyError}</p>
        {/if}

        <div class="flex gap-2">
          <button
            type="button"
            onclick={handleSurveyDismiss}
            disabled={surveySubmitting}
            class="btn-ghost flex-1 py-2 rounded-lg text-[12.5px] font-medium disabled:opacity-50"
          >
            Để sau
          </button>
          <button
            type="button"
            onclick={handleSurveySubmit}
            disabled={!surveyRating || surveySubmitting}
            class="btn-accent flex-1 py-2 rounded-lg text-[12.5px] font-semibold disabled:opacity-40"
          >
            {surveySubmitting ? "Đang gửi…" : "Gửi"}
          </button>
        </div>
      </div>
    </div>
  {/if}
</main>
