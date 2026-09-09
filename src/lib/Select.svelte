<script lang="ts">
  // Dropdown tự vẽ, thay cho <select> gốc. Lý do: popup danh sách của <select>
  // native được WebView2 render bằng UI hệ thống — CSS gần như không style
  // được (đã gặp thực tế: nền trắng chói, không khớp theme tối của app).
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import Icon from "$lib/Icon.svelte";

  interface Option {
    value: string;
    label: string;
  }
  interface Props {
    value: string;
    options: Option[];
    onchange?: (value: string) => void;
  }
  let { value = $bindable(), options, onchange }: Props = $props();

  let open = $state(false);
  let rootEl = $state<HTMLDivElement | undefined>();

  const currentLabel = $derived(options.find((o) => o.value === value)?.label ?? value);

  function select(v: string) {
    value = v;
    open = false;
    onchange?.(v);
  }

  function onDocClick(e: MouseEvent) {
    if (rootEl && !rootEl.contains(e.target as Node)) open = false;
  }

  onMount(() => {
    document.addEventListener("mousedown", onDocClick, true);
    return () => document.removeEventListener("mousedown", onDocClick, true);
  });
</script>

<div class="relative" bind:this={rootEl}>
  <button
    type="button"
    onclick={() => (open = !open)}
    class="field flex items-center justify-between gap-2 cursor-pointer"
  >
    <span>{currentLabel}</span>
    <Icon name="chevronDown" size={14} class="text-text-muted shrink-0 transition-transform {open ? 'rotate-180' : ''}" />
  </button>

  {#if open}
    <div
      class="absolute z-30 mt-1.5 w-full card overflow-hidden py-1 shadow-xl"
      transition:fade={{ duration: 110 }}
    >
      {#each options as opt (opt.value)}
        <button
          type="button"
          onclick={() => select(opt.value)}
          class="w-full text-left px-3 py-1.5 text-[13px] transition-colors {opt.value === value
            ? 'bg-accent-soft text-accent font-semibold'
            : 'text-text hover:bg-white/5'}"
        >
          {opt.label}
        </button>
      {/each}
    </div>
  {/if}
</div>
