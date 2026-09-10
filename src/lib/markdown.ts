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

/** Chuyển markdown -> văn bản THUẦN (không còn `**`, `##`, `- `...) — dùng khi
 * CHÉP câu trả lời, không phải khi hiển thị trên màn hình. Đối tượng chính
 * của app (học sinh/văn phòng) chép câu trả lời để dán thẳng vào email/Word/
 * Zalo — dán nguyên cú pháp Markdown vào đó chỉ tạo ra 1 đống ký tự rác
 * (`**quan trọng**` thay vì chữ đậm thật), không phải "an toàn hơn" hay
 * "chuẩn" hơn — cần strip sạch trước khi đưa vào clipboard.
 *
 * Cách làm: render ra HTML (đã có sẵn, dùng chung logic với hiển thị trên
 * màn hình), rồi parse lại bằng DOMParser để trích text, tự chèn xuống dòng
 * ở các thẻ block-level (thẻ Markdown->HTML không tự có \n) để giữ được cấu
 * trúc đoạn/danh sách thay vì dính liền thành 1 dòng dài. */
export function markdownToPlainText(text: string): string {
  const html = renderMarkdown(text);
  const doc = new DOMParser().parseFromString(html, "text/html");

  doc.querySelectorAll("br").forEach((el) => el.replaceWith("\n"));
  doc.querySelectorAll("li").forEach((el) => el.prepend("• "));
  doc
    .querySelectorAll("p, div, h1, h2, h3, h4, h5, h6, li, tr, blockquote, pre")
    .forEach((el) => el.append("\n"));

  const plain = (doc.body.textContent ?? "").replace(/\n{3,}/g, "\n\n").trim();
  return plain;
}
