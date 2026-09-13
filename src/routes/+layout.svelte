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
    // An toàn để apply ở MỌI cửa sổ kể cả overlay — chỉ set 1 biến CSS
    // (--chat-text-scale), cửa sổ nào không có bong bóng chat dùng biến này
    // thì không có tác dụng gì, không đụng gì tới bố cục/toạ độ chọn vùng.
    applyTextSize(loadTextSize());
  });
</script>

{@render children()}
