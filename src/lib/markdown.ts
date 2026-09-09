// Render markdown -> HTML an toàn cho tin nhắn AI (bold, list, code block,
// heading, link...). Chỉ áp dụng cho tin nhắn ĐÃ HOÀN TẤT (trong `history`),
// không áp cho phần đang stream dở — markdown chưa đóng thẻ (VD "```" mở
// chưa đóng) sẽ render vỡ, nên phần đang gõ vẫn giữ hiệu ứng reveal dạng text
// thường (xem result/+page.svelte).
import { marked } from "marked";
import DOMPurify from "dompurify";

marked.setOptions({
  breaks: true, // xuống dòng đơn (\n) -> <br>, giống cách ChatGPT/Claude hiển thị
  gfm: true, // hỗ trợ bảng, ~~gạch ngang~~, task list kiểu GitHub
});

export function renderMarkdown(text: string): string {
  const rawHtml = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(rawHtml);
}
