<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import "../app.css";
  import { applyTheme, loadTheme } from "$lib/theme";
  import { applyTextSize, loadTextSize, TEXT_SIZE_CHANGED_EVENT, type TextSizeMode } from "$lib/textSize";

  let { children } = $props();

  // Áp dụng ngay khi từng cửa sổ (main/overlay/result) mount — mỗi cửa sổ là
  // 1 document riêng nên phải tự đọc + set lại, không tự động đồng bộ giữa
  // các cửa sổ ĐANG MỞ sẵn (chỉ đúng khi mở cửa sổ mới/reload — xem thêm lý
  // do phải lắng nghe sự kiện đồng bộ real-time bên dưới cho riêng cỡ chữ).
  onMount(() => {
    applyTheme(loadTheme());
    // An toàn để apply ở MỌI cửa sổ kể cả overlay — chỉ set 1 biến CSS
    // (--chat-text-scale), cửa sổ nào không có bong bóng chat dùng biến này
    // thì không có tác dụng gì, không đụng gì tới bố cục/toạ độ chọn vùng.
    applyTextSize(loadTextSize());

    // Đổi cỡ chữ ở Settings trong lúc 1 cửa sổ "Kết quả AI"/Lịch sử khác ĐANG
    // MỞ SẴN thì localStorage không tự đẩy sang được (chỉ đọc đúng lúc mount)
    // — lỗi thực tế đã gặp: đổi xong chữ ở cửa sổ đang mở không hề đổi, trông
    // như nút "không ăn". Lắng nghe sự kiện Tauri riêng để áp dụng lại NGAY.
    const unlistenPromise = listen<TextSizeMode>(TEXT_SIZE_CHANGED_EVENT, (event) => {
      applyTextSize(event.payload);
    });

    return () => {
      unlistenPromise.then((fn) => fn());
    };
  });
</script>

{@render children()}
