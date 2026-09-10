<script lang="ts">
  import { onMount } from "svelte";
  import "../app.css";
  import { applyTheme, loadTheme } from "$lib/theme";

  let { children } = $props();

  // Áp dụng ngay khi từng cửa sổ (main/overlay/result) mount — mỗi cửa sổ là
  // 1 document riêng nên phải tự đọc + set lại, không tự động đồng bộ giữa
  // các cửa sổ ĐANG MỞ sẵn (chỉ đúng khi mở cửa sổ mới sau khi đã đổi).
  onMount(() => {
    applyTheme(loadTheme());
  });
</script>

{@render children()}
