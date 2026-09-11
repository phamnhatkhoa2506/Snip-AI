<script lang="ts">
  // Sơ đồ liên kết từ vựng — thay cho việc render markdown phẳng. Từ chính
  // ở giữa, các từ liên quan toả tròn quanh (radial layout tự vẽ bằng SVG,
  // không cần lib đồ thị — chỉ 3-5 node, tính toạ độ bằng lượng giác đơn giản
  // là đủ). Bấm vào 1 từ liên quan thì hiện ví dụ của từ đó bên dưới sơ đồ.
  //
  // Chữ nằm trong <foreignObject> (HTML thật, không phải <text> SVG) — lý do:
  // <text> SVG không tự xuống dòng/co chữ theo độ dài, chữ dài (VD "cloud
  // infrastructure") tràn hẳn ra ngoài vòng tròn. foreignObject cho phép dùng
  // CSS bình thường (wrap, line-clamp, font-size theo độ dài) để chữ LUÔN nằm
  // gọn trong node, bất kể ngắn hay dài.
  import Icon from "$lib/Icon.svelte";

  interface DiagramTerm {
    term: string;
    translation: string;
    relation: string;
    example: string;
  }
  interface DiagramData {
    mainTerm: string;
    translation: string;
    related: DiagramTerm[];
  }

  let { data }: { data: DiagramData } = $props();

  const SIZE = 380;
  const CENTER = SIZE / 2;
  const RADIUS = 152;
  const NODE_R = 42;
  const MAIN_R = 50;
  // Khoảng trống THẬT giữa mép vòng tròn giữa và mép node — nhãn quan hệ
  // PHẢI nằm lọt trong khoảng này, không thì bị 2 vòng tròn đè lên (bug đã
  // gặp: nhãn đặt ở giữa ĐƯỜNG NỐI theo tỉ lệ %, nhưng node to gần chạm nhau
  // nên "giữa đường nối" thực ra vẫn nằm TRONG vòng tròn của center/node).
  const GAP = RADIUS - MAIN_R - NODE_R;

  const positions = $derived(
    data.related.map((_, i) => {
      // Bắt đầu từ hướng "12 giờ" (-90°), rải đều quanh vòng tròn.
      const angle = -Math.PI / 2 + (2 * Math.PI * i) / data.related.length;
      return {
        angle,
        x: CENTER + RADIUS * Math.cos(angle),
        y: CENTER + RADIUS * Math.sin(angle),
      };
    }),
  );

  /** Điểm đặt nhãn quan hệ — đúng CHÍNH GIỮA khoảng trống giữa 2 vòng tròn
   * (không phải giữa đường nối), nên luôn lộ ra, không bị node nào che. */
  const labelPositions = $derived(
    positions.map((p) => {
      const dist = MAIN_R + GAP / 2;
      return { x: CENTER + dist * Math.cos(p.angle), y: CENTER + dist * Math.sin(p.angle) };
    }),
  );

  /** Cỡ chữ co lại dần theo độ dài — chữ ngắn ("fluke") vẫn to dễ đọc, chữ
   * dài ("infrastructure") không bị vỡ layout. */
  function termFontSize(text: string): number {
    if (text.length > 16) return 8.5;
    if (text.length > 11) return 9.5;
    if (text.length > 7) return 10.5;
    return 11.5;
  }

  let selected = $state<number | null>(null);
  const activeExample = $derived(selected !== null ? data.related[selected] : null);
</script>

<div class="flex flex-col items-center gap-3">
  <svg viewBox="0 0 {SIZE} {SIZE}" class="w-full max-w-[380px] aspect-square select-none overflow-visible">
    <!-- Đường nối — vẽ TRƯỚC node để node đè lên, không bị đường kẻ cắt ngang mặt chữ.
    Màu trung tính (theo border của app, không phải accent) — giữ sơ đồ điềm
    tĩnh, chỉ node được chọn mới nổi bật lên bằng accent. -->
    {#each positions as pos, i (i)}
      <line
        x1={CENTER}
        y1={CENTER}
        x2={pos.x}
        y2={pos.y}
        stroke={selected === i ? "var(--color-accent)" : "var(--color-border)"}
        stroke-width={selected === i ? 2 : 1.3}
        opacity={selected === null || selected === i ? 1 : 0.3}
      />
    {/each}

    <!-- Nhãn quan hệ — đặt đúng giữa KHOẢNG TRỐNG giữa 2 vòng tròn (xem
    labelPositions), không phải giữa đường nối, nên không bao giờ bị node đè
    lên. Cho phép xuống tối đa 2 dòng (line-clamp) thay vì ép 1 dòng + "…" —
    ép 1 dòng với chữ căn giữa (text-align:center) làm chữ bị CẮT MẤT CẢ 2 ĐẦU
    (kể cả chữ cái đầu) một cách vô lý thay vì cắt gọn ở cuối kèm dấu "…", vì
    phần chữ tràn ra tràn đều 2 bên tâm — lỗi thực tế đã gặp. Xuống dòng tự
    nhiên fit đúng khoảng trống hẹp mà không mất chữ. -->
    {#each labelPositions as lp, i (i)}
      <g opacity={selected === null || selected === i ? 1 : 0.3}>
        <foreignObject x={lp.x - 27} y={lp.y - 12} width="54" height="24">
          <div
            class="flex items-center justify-center h-full text-center leading-tight px-1 rounded"
            style="font-size: 7px; color: var(--color-text-muted); background: color-mix(in srgb, var(--color-bg-elevated) 85%, transparent);"
          >
            <span style="display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;">
              {data.related[i].relation}
            </span>
          </div>
        </foreignObject>
      </g>
    {/each}

    <!-- Node từ liên quan -->
    {#each positions as pos, i (i)}
      <g
        role="button"
        tabindex="0"
        onclick={() => (selected = selected === i ? null : i)}
        onkeydown={(e) => e.key === "Enter" && (selected = selected === i ? null : i)}
        class="cursor-pointer"
        opacity={selected === null || selected === i ? 1 : 0.5}
      >
        {#if selected === i}
          <circle cx={pos.x} cy={pos.y} r={NODE_R} fill="url(#mainGrad)" />
        {:else}
          <circle cx={pos.x} cy={pos.y} r={NODE_R} fill="var(--color-bg-elevated)" stroke="var(--color-border)" stroke-width="1.3" />
        {/if}
        <foreignObject x={pos.x - NODE_R * 0.82} y={pos.y - NODE_R * 0.82} width={NODE_R * 1.64} height={NODE_R * 1.64}>
          <div class="flex flex-col items-center justify-center h-full text-center gap-0.5 px-1 overflow-hidden">
            <span
              class="font-semibold leading-tight"
              style="font-size: {termFontSize(data.related[i].term)}px; color: {selected === i
                ? 'var(--color-accent-text)'
                : 'var(--color-text)'}; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;"
            >
              {data.related[i].term}
            </span>
            <span
              class="leading-tight opacity-90"
              style="font-size: 8px; color: {selected === i ? 'var(--color-accent-text)' : 'var(--color-text-muted)'}; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;"
            >
              {data.related[i].translation}
            </span>
          </div>
        </foreignObject>
      </g>
    {/each}

    <!-- Từ chính, ở giữa -->
    <circle cx={CENTER} cy={CENTER} r={MAIN_R} fill="url(#mainGrad)" />
    <defs>
      <linearGradient id="mainGrad" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0%" stop-color="var(--color-accent)" />
        <stop offset="100%" stop-color="var(--color-accent-2)" />
      </linearGradient>
    </defs>
    <foreignObject x={CENTER - MAIN_R * 0.85} y={CENTER - MAIN_R * 0.85} width={MAIN_R * 1.7} height={MAIN_R * 1.7}>
      <div class="flex flex-col items-center justify-center h-full text-center gap-0.5 px-1 overflow-hidden">
        <span
          class="font-bold leading-tight"
          style="font-size: {termFontSize(data.mainTerm) + 2}px; color: var(--color-accent-text); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;"
        >
          {data.mainTerm}
        </span>
        <span
          class="leading-tight"
          style="font-size: 9px; color: var(--color-accent-text); opacity: 0.9; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;"
        >
          {data.translation}
        </span>
      </div>
    </foreignObject>
  </svg>

  {#if activeExample}
    <div class="card w-full px-3 py-2.5 text-[12px] leading-relaxed flex items-start gap-2">
      <Icon name="quote" size={13} class="shrink-0 mt-0.5 text-accent" />
      <div>
        <div class="font-semibold text-[12.5px]">{activeExample.term} <span class="text-text-muted font-normal">— {activeExample.translation}</span></div>
        <div class="text-text-muted italic mt-0.5">{activeExample.example}</div>
      </div>
    </div>
  {:else}
    <div class="text-[10.5px] text-text-muted">Bấm vào 1 từ liên quan để xem ví dụ</div>
  {/if}
</div>
