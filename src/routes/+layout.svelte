<script lang="ts">
  import { onMount } from "svelte";
  import "../app.css";
  import { applyTheme, loadTheme } from "$lib/theme";
  import { applyTextSize, loadTextSize } from "$lib/textSize";

  let { children } = $props();

  // Áp dụng ngay khi từng cửa sổ (main/overlay/result) mount — mỗi cửa sổ là
  // 1 document riêng nên phải tự đọc + set lại, không tự động đồng bộ giữa
  // các cửa sổ ĐANG MỞ sẵn (chỉ đúng khi mở cửa sổ mới sau khi đã đổi).
  onMount(() => {
    applyTheme(loadTheme());

    // Cỡ chữ (zoom) CỐ TÌNH KHÔNG áp dụng cho cửa sổ "overlay" — overlay tính
    // khung chọn vùng chụp trực tiếp từ e.clientX/e.clientY để suy ra toạ độ
    // pixel THẬT trên màn hình (xem overlay/+page.svelte); zoom sẽ làm lệch
    // phép tính đó, có thể chụp sai vùng. Xem giải thích đầy đủ ở textSize.ts.
    if (!window.location.pathname.startsWith("/overlay")) {
      applyTextSize(loadTextSize());
    }
  });
</script>

{@render children()}
