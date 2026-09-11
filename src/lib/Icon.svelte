<script lang="ts">
  // Bộ icon SVG dạng line (stroke) thống nhất, thay cho emoji dùng trước đây —
  // emoji render khác nhau tuỳ font/OS và trông lệch tông với giao diện tối.
  // Tất cả icon dùng chung viewBox 24x24, stroke currentColor, nên đổi màu/kích
  // thước chỉ bằng class Tailwind bên ngoài (text-*, w-*, h-*).
  interface Props {
    name: string;
    size?: number;
    class?: string;
    strokeWidth?: number;
  }
  let { name, size = 16, class: className = "", strokeWidth = 2 }: Props = $props();

  // Mỗi icon là danh sách phần tử SVG con
  type Shape =
    | { t: "path"; d: string }
    | { t: "circle"; cx: number; cy: number; r: number }
    | { t: "rect"; x: number; y: number; w: number; h: number; rx: number };

  const ICONS: Record<string, Shape[]> = {
    // Khung chọn vùng — biểu tượng chính của app
    scan: [
      { t: "path", d: "M3 7V5a2 2 0 0 1 2-2h2" },
      { t: "path", d: "M17 3h2a2 2 0 0 1 2 2v2" },
      { t: "path", d: "M21 17v2a2 2 0 0 1-2 2h-2" },
      { t: "path", d: "M7 21H5a2 2 0 0 1-2-2v-2" },
      { t: "circle", cx: 12, cy: 12, r: 3 },
    ],
    // Avatar AI (thay cho emoji 🤖)
    sparkles: [
      {
        t: "path",
        d: "M11.5 3.2a.5.5 0 0 1 1 0l1.3 4.1a2 2 0 0 0 1.4 1.4l4.1 1.3a.5.5 0 0 1 0 1l-4.1 1.3a2 2 0 0 0-1.4 1.4l-1.3 4.1a.5.5 0 0 1-1 0l-1.3-4.1a2 2 0 0 0-1.4-1.4l-4.1-1.3a.5.5 0 0 1 0-1l4.1-1.3a2 2 0 0 0 1.4-1.4z",
      },
      { t: "path", d: "M19 3v3" },
      { t: "path", d: "M20.5 4.5h-3" },
      { t: "path", d: "M5 18v3" },
      { t: "path", d: "M6.5 19.5h-3" },
    ],
    text: [
      { t: "path", d: "M17 6.1H3" },
      { t: "path", d: "M21 12.1H3" },
      { t: "path", d: "M15.1 18H3" },
    ],
    languages: [
      { t: "path", d: "m5 8 6 6" },
      { t: "path", d: "m4 14 6-6 2-3" },
      { t: "path", d: "M2 5h12" },
      { t: "path", d: "M7 2h1" },
      { t: "path", d: "m22 22-5-10-5 10" },
      { t: "path", d: "M14 18h6" },
    ],
    list: [
      { t: "path", d: "M8 6h13" },
      { t: "path", d: "M8 12h13" },
      { t: "path", d: "M8 18h13" },
      { t: "path", d: "M3 6h.01" },
      { t: "path", d: "M3 12h.01" },
      { t: "path", d: "M3 18h.01" },
    ],
    lightbulb: [
      {
        t: "path",
        d: "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5a6 6 0 0 0-12 0c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5",
      },
      { t: "path", d: "M9 18h6" },
      { t: "path", d: "M10 22h4" },
    ],
    code: [
      { t: "path", d: "m16 18 6-6-6-6" },
      { t: "path", d: "m8 6-6 6 6 6" },
    ],
    copy: [
      { t: "rect", x: 8, y: 8, w: 14, h: 14, rx: 2 },
      { t: "path", d: "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" },
    ],
    check: [{ t: "path", d: "M20 6 9 17l-5-5" }],
    // Dấu cộng vẽ bằng SVG (không dùng ký tự "+" của font): ký tự văn bản có
    // đường baseline riêng nên luôn lệch nhẹ so với chữ bên cạnh, và độ dày
    // nét đổi theo font hệ thống. Vẽ bằng path thì luôn cân giữa và cùng
    // stroke-width với mọi icon khác.
    plus: [
      { t: "path", d: "M12 5v14" },
      { t: "path", d: "M5 12h14" },
    ],
    // Vòng xoay chờ — cung hở 3/4 để khi quay (animate-spin) nhìn thấy rõ
    // chuyển động; vòng tròn kín thì quay mà trông như đứng yên.
    loader: [{ t: "path", d: "M21 12a9 9 0 1 1-6.22-8.56" }],
    // Con trỏ chuột — chip "Thao tác" của phiên video (mô tả các bước đã làm)
    "mouse-pointer": [
      { t: "path", d: "M3 3l7.07 16.97 2.51-7.39 7.39-2.51z" },
      { t: "path", d: "M13 13l6 6" },
    ],
    // Mục tiêu — thanh chọn thời điểm/khoảng cụ thể trong video
    target: [
      { t: "circle", cx: 12, cy: 12, r: 9 },
      { t: "circle", cx: 12, cy: 12, r: 5 },
      { t: "circle", cx: 12, cy: 12, r: 1 },
    ],
    // Đồng hồ — nút mở Lịch sử
    clock: [
      { t: "circle", cx: 12, cy: 12, r: 9 },
      { t: "path", d: "M12 7v5l3.5 2" },
    ],
    // Ảnh — mục lịch sử kiểu snip ảnh (khác "video")
    image: [
      { t: "rect", x: 3, y: 3, w: 18, h: 18, rx: 3 },
      { t: "circle", cx: 8.5, cy: 8.5, r: 1.7 },
      { t: "path", d: "M21 15l-5-5L5 21" },
    ],
    x: [
      { t: "path", d: "M18 6 6 18" },
      { t: "path", d: "m6 6 12 12" },
    ],
    send: [
      { t: "path", d: "M14.5 4.5 21 3l-1.5 6.5" },
      { t: "path", d: "M21 3 10 14" },
      { t: "path", d: "M21 3 14 21l-4-7-7-4z" },
    ],
    settings: [
      { t: "path", d: "M4 21v-7" },
      { t: "path", d: "M4 10V3" },
      { t: "path", d: "M12 21v-9" },
      { t: "path", d: "M12 8V3" },
      { t: "path", d: "M20 21v-5" },
      { t: "path", d: "M20 12V3" },
      { t: "path", d: "M2 14h4" },
      { t: "path", d: "M10 8h4" },
      { t: "path", d: "M18 16h4" },
    ],
    lock: [
      { t: "rect", x: 3, y: 11, w: 18, h: 11, rx: 2 },
      { t: "path", d: "M7 11V7a5 5 0 0 1 10 0v4" },
    ],
    shield: [
      {
        t: "path",
        d: "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z",
      },
      { t: "path", d: "m9 12 2 2 4-4" },
    ],
    chevronDown: [{ t: "path", d: "m6 9 6 6 6-6" }],
    trash: [
      { t: "path", d: "M3 6h18" },
      { t: "path", d: "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" },
      { t: "path", d: "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" },
    ],
    eye: [
      { t: "path", d: "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7-10-7-10-7z" },
      { t: "circle", cx: 12, cy: 12, r: 3 },
    ],
    eyeOff: [
      { t: "path", d: "M10.7 5.1A9.9 9.9 0 0 1 12 5c6.4 0 10 7 10 7a17 17 0 0 1-2.4 3.3" },
      { t: "path", d: "M6.6 6.6A17 17 0 0 0 2 12s3.6 7 10 7a9.7 9.7 0 0 0 5.4-1.6" },
      { t: "path", d: "m2 2 20 20" },
      { t: "path", d: "M9.9 9.9a3 3 0 0 0 4.2 4.2" },
    ],
    camera: [
      { t: "path", d: "M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3z" },
      { t: "circle", cx: 12, cy: 13, r: 3 },
    ],
    video: [
      { t: "path", d: "m22 8-6 4 6 4V8Z" },
      { t: "rect", x: 2, y: 6, w: 14, h: 12, rx: 2 },
    ],
    globe: [
      { t: "circle", cx: 12, cy: 12, r: 10 },
      { t: "path", d: "M2 12h20" },
      { t: "path", d: "M12 2a15 15 0 0 1 0 20 15 15 0 0 1 0-20z" },
    ],
    keyboard: [
      { t: "rect", x: 2, y: 6, w: 20, h: 13, rx: 2 },
      { t: "path", d: "M6 10h.01" },
      { t: "path", d: "M10 10h.01" },
      { t: "path", d: "M14 10h.01" },
      { t: "path", d: "M18 10h.01" },
      { t: "path", d: "M8 15h8" },
    ],
    plug: [
      { t: "path", d: "M12 22v-5" },
      { t: "path", d: "M9 8V2" },
      { t: "path", d: "M15 8V2" },
      { t: "path", d: "M18 8v3a6 6 0 0 1-12 0V8z" },
    ],
    alert: [
      { t: "circle", cx: 12, cy: 12, r: 10 },
      { t: "path", d: "M12 8v4" },
      { t: "path", d: "M12 16h.01" },
    ],
    edit: [
      { t: "path", d: "M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" },
      { t: "path", d: "M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4z" },
    ],
    // 3 icon cho công tắc theme sáng/tối/hệ thống
    sun: [
      { t: "circle", cx: 12, cy: 12, r: 4 },
      { t: "path", d: "M12 2v2" },
      { t: "path", d: "M12 20v2" },
      { t: "path", d: "m4.9 4.9 1.4 1.4" },
      { t: "path", d: "m17.7 17.7 1.4 1.4" },
      { t: "path", d: "M2 12h2" },
      { t: "path", d: "M20 12h2" },
      { t: "path", d: "m6.3 17.7-1.4 1.4" },
      { t: "path", d: "m19.1 4.9-1.4 1.4" },
    ],
    moon: [{ t: "path", d: "M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z" }],
    monitor: [
      { t: "rect", x: 2, y: 4, w: 20, h: 13, rx: 2 },
      { t: "path", d: "M8 21h8" },
      { t: "path", d: "M12 17v4" },
    ],
    // Dấu trích dẫn — ví dụ câu trong sơ đồ từ vựng
    quote: [
      { t: "path", d: "M9 6H5a1 1 0 0 0-1 1v4a1 1 0 0 0 1 1h3v1c0 1.5-1 2.5-2.5 2.5" },
      { t: "path", d: "M19 6h-4a1 1 0 0 0-1 1v4a1 1 0 0 0 1 1h3v1c0 1.5-1 2.5-2.5 2.5" },
    ],
    // Kính lúp có dấu "+" — nút phóng to sơ đồ từ vựng
    zoomIn: [
      { t: "circle", cx: 11, cy: 11, r: 7 },
      { t: "path", d: "m21 21-4.3-4.3" },
      { t: "path", d: "M11 8v6" },
      { t: "path", d: "M8 11h6" },
    ],
    // Mạng lưới nút liên kết — chip "Sơ đồ từ vựng"
    network: [
      { t: "circle", cx: 12, cy: 5, r: 2.5 },
      { t: "circle", cx: 5, cy: 19, r: 2.5 },
      { t: "circle", cx: 19, cy: 19, r: 2.5 },
      { t: "path", d: "M12 7.5v6" },
      { t: "path", d: "M10.2 12 6.8 17" },
      { t: "path", d: "M13.8 12l3.4 5" },
    ],
  };

  const shapes = $derived(ICONS[name] ?? []);
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  class={className}
  aria-hidden="true"
>
  {#each shapes as shape, i (i)}
    {#if shape.t === "path"}
      <path d={shape.d} />
    {:else if shape.t === "circle"}
      <circle cx={shape.cx} cy={shape.cy} r={shape.r} />
    {:else}
      <rect x={shape.x} y={shape.y} width={shape.w} height={shape.h} rx={shape.rx} />
    {/if}
  {/each}
</svg>
