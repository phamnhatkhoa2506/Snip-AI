<script lang="ts">
  import { onMount } from "svelte";
  import { fade, slide } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";
  import { REASONING_EFFORT_PRESETS } from "$lib/config";
  import { captureCombo, getHotkey, setHotkey } from "$lib/hotkey";
  import {
    DEFAULT_SETTINGS,
    PROVIDERS,
    currentModel,
    deleteApiKey,
    fetchKeyStatuses,
    loadSettings,
    providerMeta,
    saveApiKey,
    saveSettings,
    setCurrentModel,
    type KeyStatus,
    type Provider,
    type Settings,
  } from "$lib/settings";

  let settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  let keyStatuses = $state<Partial<Record<Provider, KeyStatus>>>({});
  let keyInput = $state("");
  let showKey = $state(false);
  let editingKey = $state(false);
  let busy = $state(false);
  let toast = $state<{ kind: "ok" | "err"; text: string } | null>(null);
  let captureError = $state("");

  // ── Phím tắt ──────────────────────────────────────────────────────────
  let hotkeyParts = $state<string[]>(["Ctrl", "PrintScreen"]);
  let recordingHotkey = $state(false);
  let hotkeyBusy = $state(false);
  let hotkeyError = $state("");

  async function loadHotkey() {
    try {
      const accel = await getHotkey();
      hotkeyParts = accel.split("+");
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
      hotkeyParts = confirmed.split("+");
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

  const meta = $derived(providerMeta(settings.provider));
  const status = $derived(keyStatuses[settings.provider]);

  function flash(kind: "ok" | "err", text: string) {
    toast = { kind, text };
    setTimeout(() => (toast = null), 2600);
  }

  async function refreshKeys() {
    try {
      keyStatuses = await fetchKeyStatuses();
    } catch (e) {
      flash("err", `Không đọc được trạng thái key: ${e}`);
    }
  }

  onMount(() => {
    // onMount không cho callback async trả về hàm dọn dẹp (Promise != function),
    // nên tách phần load dữ liệu ra 1 hàm async gọi rời, còn onMount chỉ trả
    // thẳng cleanup function.
    settings = loadSettings();
    refreshKeys();
    loadHotkey();
    return () => stopRecording(); // dọn listener nếu rời trang giữa lúc đang ghi phím
  });

  function switchProvider(p: Provider) {
    settings.provider = p;
    keyInput = "";
    editingKey = false;
    showKey = false;
    saveSettings(settings);
  }

  function persistPrefs() {
    saveSettings(settings);
    flash("ok", "Đã lưu cài đặt");
  }

  async function handleSaveKey() {
    const key = keyInput.trim();
    if (!key) {
      flash("err", "Chưa nhập API key");
      return;
    }
    busy = true;
    try {
      await saveApiKey(settings.provider, key);
      keyInput = "";
      editingKey = false;
      showKey = false;
      await refreshKeys();
      flash("ok", `Đã lưu key ${meta.label} vào Windows Credential Manager`);
    } catch (e) {
      flash("err", String(e));
    } finally {
      busy = false;
    }
  }

  async function handleDeleteKey() {
    busy = true;
    try {
      await deleteApiKey(settings.provider);
      await refreshKeys();
      flash("ok", `Đã xoá key ${meta.label} khỏi máy`);
    } catch (e) {
      flash("err", String(e));
    } finally {
      busy = false;
    }
  }

  async function handleTestCapture() {
    captureError = "";
    try {
      await invoke("trigger_capture");
    } catch (e) {
      captureError = String(e);
    }
  }
</script>

<main class="app-bg min-h-screen text-text flex flex-col">
  <!-- Top bar -->
  <header class="glass sticky top-0 z-10 px-5 py-3.5 flex items-center gap-3">
    <div
      class="w-8 h-8 rounded-xl flex items-center justify-center text-black"
      style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
    >
      <Icon name="scan" size={17} strokeWidth={2.4} />
    </div>
    <div class="flex-1 min-w-0">
      <h1 class="text-[15px] font-bold leading-tight">Snip-AI</h1>
      <p class="text-[11px] text-text-muted leading-tight">Chụp màn hình · Hỏi AI</p>
    </div>
    <span class="badge badge-accent"><Icon name="shield" size={11} /> Key mã hoá bởi Windows</span>
  </header>

  <div class="flex-1 overflow-y-auto p-5 flex flex-col gap-4">
    <!-- Hotkey -->
    <div class="card px-4 py-3 flex flex-col gap-2.5">
      <div class="flex items-center gap-3">
        <Icon name="keyboard" size={16} class="text-accent shrink-0" />
        <div class="flex-1 flex items-center gap-1 flex-wrap min-w-0">
          {#if recordingHotkey}
            <span class="text-[12.5px] text-accent font-medium animate-pulse">Nhấn tổ hợp phím mới…</span>
          {:else}
            {#each hotkeyParts as part, i (i)}
              {#if i > 0}<span class="text-text-muted mx-0.5 text-[11px]">+</span>{/if}
              <kbd class="px-1.5 py-0.5 rounded-md bg-bg-elevated border border-border text-[11px] text-text font-mono"
                >{part}</kbd
              >
            {/each}
          {/if}
        </div>
        <button
          onclick={() => (recordingHotkey ? stopRecording() : startRecording())}
          disabled={hotkeyBusy}
          class="btn-ghost px-3 py-1.5 rounded-lg text-[12px] font-medium flex items-center gap-1.5 shrink-0 disabled:opacity-50"
        >
          {#if recordingHotkey}
            <Icon name="x" size={13} /> Huỷ
          {:else}
            <Icon name="edit" size={13} /> Đổi
          {/if}
        </button>
      </div>
      {#if hotkeyError}
        <p class="text-[11.5px] text-[color:var(--color-danger)] selectable leading-relaxed" transition:fade={{ duration: 140 }}>
          {hotkeyError}
        </p>
      {/if}
    </div>

    <!-- Provider -->
    <section class="card p-4 flex flex-col gap-3.5">
      <h2 class="text-[11px] font-bold text-text-muted uppercase tracking-wider">Nhà cung cấp AI</h2>

      <div class="grid grid-cols-2 gap-2">
        {#each PROVIDERS as p (p.id)}
          {@const active = settings.provider === p.id}
          {@const hasKey = keyStatuses[p.id]?.hasKey}
          <button
            onclick={() => switchProvider(p.id)}
            class="relative px-3 py-2.5 rounded-xl text-[13px] font-semibold text-left transition-all duration-150 border {active
              ? 'border-transparent text-black'
              : 'border-border text-text-muted hover:text-text hover:border-[#3a424b]'}"
            style={active
              ? "background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
              : "background: rgba(255,255,255,0.035);"}
          >
            {p.label}
            {#if hasKey}
              <span
                class="absolute top-2 right-2 w-1.5 h-1.5 rounded-full {active ? 'bg-black/50' : 'bg-accent'}"
                title="Đã có API key"
              ></span>
            {/if}
          </button>
        {/each}
      </div>

      <!-- API key -->
      <div class="flex flex-col gap-2 pt-1">
        <div class="flex items-center gap-2">
          <Icon name="lock" size={13} class="text-text-muted" />
          <span class="text-[12.5px] text-text-muted flex-1">API Key</span>
          {#if status?.hasKey && !editingKey}
            <span class="badge badge-accent"><Icon name="check" size={10} /> Đã lưu {status.hint}</span>
          {/if}
        </div>

        {#if status?.hasKey && !editingKey}
          <div class="flex gap-2" transition:fade={{ duration: 140 }}>
            <div
              class="field selectable flex-1 flex items-center font-mono text-text-muted tracking-widest"
              aria-label="API key đã được lưu, không thể xem lại"
            >
              ••••••••••••••••{status.hint}
            </div>
            <button
              onclick={() => {
                editingKey = true;
                keyInput = "";
              }}
              class="btn-ghost px-3 rounded-lg text-[12.5px] font-medium flex items-center gap-1.5"
              title="Thay key mới"
            >
              <Icon name="edit" size={14} /> Đổi
            </button>
            <button
              onclick={handleDeleteKey}
              disabled={busy}
              class="btn-ghost px-3 rounded-lg text-[12.5px] font-medium flex items-center gap-1.5 hover:!text-[color:var(--color-danger)]"
              title="Xoá key khỏi Windows Credential Manager"
            >
              <Icon name="trash" size={14} />
            </button>
          </div>
        {:else}
          <div class="flex flex-col gap-2" transition:slide={{ duration: 160 }}>
            <div class="flex gap-2">
              <div class="relative flex-1">
                <input
                  type={showKey ? "text" : "password"}
                  bind:value={keyInput}
                  placeholder={meta.keyPlaceholder}
                  autocomplete="off"
                  spellcheck="false"
                  class="field selectable pr-9 font-mono"
                  onkeydown={(e) => e.key === "Enter" && handleSaveKey()}
                />
                <button
                  onclick={() => (showKey = !showKey)}
                  class="absolute right-2 top-1/2 -translate-y-1/2 p-1 rounded text-text-muted hover:text-text transition-colors"
                  title={showKey ? "Ẩn key" : "Hiện key"}
                >
                  <Icon name={showKey ? "eyeOff" : "eye"} size={15} />
                </button>
              </div>
              <button
                onclick={handleSaveKey}
                disabled={busy}
                class="btn-accent px-4 rounded-lg text-[12.5px]">Lưu key</button
              >
              {#if editingKey}
                <button
                  onclick={() => {
                    editingKey = false;
                    keyInput = "";
                  }}
                  class="btn-ghost px-3 rounded-lg text-[12.5px]"><Icon name="x" size={14} /></button
                >
              {/if}
            </div>
            <p class="text-[11px] text-text-muted leading-relaxed">
              Lấy key tại <span class="text-text">{meta.keyUrl}</span>. Key được lưu vào Windows Credential
              Manager, mã hoá theo tài khoản Windows của bạn — app không lưu key dạng chữ thường và không thể
              đọc lại key sau khi lưu.
            </p>
          </div>
        {/if}
      </div>

      <!-- Model -->
      <label class="flex flex-col gap-1.5 text-[12.5px]">
        <span class="text-text-muted">Model (Vision Model)</span>
        <input
          type="text"
          value={currentModel(settings)}
          oninput={(e) => setCurrentModel(settings, e.currentTarget.value)}
          onchange={persistPrefs}
          class="field selectable font-mono"
          spellcheck="false"
        />
      </label>

      {#if settings.provider === "nvidia"}
        <div class="flex flex-col gap-2" transition:slide={{ duration: 160 }}>
          <button
            type="button"
            onclick={() => {
              settings.nvidiaReasoningEnabled = !settings.nvidiaReasoningEnabled;
              persistPrefs();
            }}
            class="flex items-center gap-2.5 text-left"
          >
            <span
              class="relative w-8 h-[18px] rounded-full shrink-0 transition-colors duration-150 {settings.nvidiaReasoningEnabled
                ? ''
                : 'bg-bg-elevated border border-border'}"
              style={settings.nvidiaReasoningEnabled
                ? "background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
                : ""}
            >
              <span
                class="absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white transition-all duration-150 {settings.nvidiaReasoningEnabled
                  ? 'left-[17px]'
                  : 'left-0.5'}"
              ></span>
            </span>
            <span class="text-[12.5px] text-text">Reasoning Effort (DeepSeek, Kimi, ...)</span>
          </button>

          {#if settings.nvidiaReasoningEnabled}
            <div class="flex flex-col gap-1.5 text-[12.5px] pl-[42px]" transition:slide={{ duration: 140 }}>
              <input
                type="text"
                bind:value={settings.nvidiaReasoningEffort}
                onchange={persistPrefs}
                placeholder="VD: none, low, medium, high, max"
                class="field selectable font-mono"
                spellcheck="false"
              />
              <div class="flex flex-wrap gap-1.5">
                {#each REASONING_EFFORT_PRESETS as preset (preset)}
                  <button
                    type="button"
                    class="chip !text-[11px] !py-1"
                    onclick={() => {
                      settings.nvidiaReasoningEffort = preset;
                      persistPrefs();
                    }}
                  >
                    {preset}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    </section>

  </div>

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
