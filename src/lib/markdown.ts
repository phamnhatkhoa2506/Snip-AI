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
  // `ADD_TAGS`/`ADD_ATTR`: đảm bảo <button data-ts> (dùng cho mốc giờ video
  // bấm được, xem linkifyTimestamps bên dưới) LUÔN sống sót qua sanitize —
  // không dựa vào việc DOMPurify mặc định có cho phép "button"/"data-ts" hay
  // không (không nên đoán, khai báo rõ cho chắc).
  return DOMPurify.sanitize(rawHtml, { ADD_TAGS: ["button"], ADD_ATTR: ["data-ts"] });
}

/** Biến mốc giờ dạng "[mm:ss]" trong câu trả lời VIDEO thành nút bấm được —
 * bấm vào là video tự tua tới đúng giây đó, biến câu trả lời thành 1 "mục
 * lục bấm được" thay vì đoạn văn tĩnh (xem result/+page.svelte, nơi gắn sự
 * kiện click + gọi `videoEl.currentTime`).
 *
 * PHẢI gọi hàm này TRƯỚC `renderMarkdown` (trên text markdown thô), rồi mới
 * đưa kết quả vào `renderMarkdown` — không gộp thẳng vào `renderMarkdown`
 * dùng chung vì phiên ẢNH không bao giờ có mốc giờ, không cần tốn công chạy
 * regex này cho mọi câu trả lời.
 *
 * Không dùng inline `onclick="..."` trong chuỗi HTML — DOMPurify (chạy bên
 * trong `renderMarkdown` ngay sau) MẶC ĐỊNH XOÁ mọi thuộc tính `on*` để chặn
 * XSS, gắn onclick trực tiếp ở đây sẽ bị xoá mất, bấm vào không có tác dụng
 * gì. Thay vào đó chỉ đánh dấu bằng class + data-ts; phía Svelte lắng nghe 1
 * sự kiện click DUY NHẤT ở khối bọc ngoài rồi tự dò đúng nút vừa bấm. */
export function linkifyTimestamps(text: string): string {
  // Bóc dấu backtick bao quanh mốc giờ TRƯỚC — model (theo đúng quy tắc #4
  // trong SYSTEM_PROMPT: dùng `code` cho "giá trị kỹ thuật") hay tự bọc
  // "[mm:ss]" trong backtick. Nếu không bóc, <button> chèn vào bên dưới sẽ
  // nằm LỌT VÀO TRONG 1 code-span markdown — mà code-span luôn hiện HTML dạng
  // chữ thô (không render thành thẻ thật), đúng lỗi thực tế đã gặp: người
  // dùng thấy nguyên văn `<button ...>[00:00]</button>` thay vì nút bấm được.
  const withoutBackticks = text.replace(/`(\[\d{1,2}:[0-5]\d\])`/g, "$1");
  return withoutBackticks.replace(/\[(\d{1,2}):([0-5]\d)\]/g, (match, mm: string, ss: string) => {
    const seconds = Number(mm) * 60 + Number(ss);
    return `<button type="button" class="ts-link" data-ts="${seconds}">${match}</button>`;
  });
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
