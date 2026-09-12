// Render markdown -> HTML an toàn cho tin nhắn AI (bold, list, code block,
// heading, link, công thức toán...). Chỉ áp dụng cho tin nhắn ĐÃ HOÀN TẤT
// (trong `history`), không áp cho phần đang stream dở — markdown chưa đóng
// thẻ (VD "```" mở chưa đóng) sẽ render vỡ, nên phần đang gõ vẫn giữ hiệu
// ứng reveal dạng text thường (xem result/+page.svelte).
import { marked } from "marked";
import DOMPurify from "dompurify";
import katex from "katex";

marked.setOptions({
  breaks: true, // xuống dòng đơn (\n) -> <br>, giống cách ChatGPT/Claude hiển thị
  gfm: true, // hỗ trợ bảng, ~~gạch ngang~~, task list kiểu GitHub
});

// ─────────────────────────────────────────────────────────────────────────
// TIỀN XỬ LÝ trước khi đưa qua `marked` — 3 lỗi thực tế phát hiện qua test
// trực tiếp với thư viện (không phải đoán): công thức LaTeX hiện nguyên văn
// (hoặc còn bị hỏng thêm), phép nhân "*" bị hiểu nhầm thành in nghiêng, và
// placeholder/thẻ dạng "<...>" viết ngoài code bị NUỐT MẤT hoàn toàn (không
// phải chỉ hiển thị sai — biến mất khỏi màn hình, không dấu vết). Xử lý CẢ 3
// TRƯỚC khi gọi marked, và CHỈ trên phần văn bản THƯỜNG — bên trong code
// block/inline code (```...```, `...`) giữ nguyên 100%, không đụng vào,
// vì đó là nơi người dùng/AI cố ý muốn hiện đúng nguyên văn ký tự gốc.
// ─────────────────────────────────────────────────────────────────────────

/** Tách text thành các đoạn "code" (giữ nguyên, không xử lý gì) và "thường"
 * (sẽ qua các bước xử lý bên dưới) — dùng `.split()` với regex có nhóm bắt để
 * giữ lại cả phần khớp lẫn không khớp, xen kẽ nhau trong mảng kết quả. */
function splitCodeSegments(text: string): { code: boolean; text: string }[] {
  const parts = text.split(/(```[\s\S]*?```|`[^`\n]*`)/g);
  return parts.map((part, i) => ({ code: i % 2 === 1, text: part }));
}

/** Công thức toán trích ra được thay bằng 1 token vô hại (chỉ chữ+số, không
 * có ký tự markdown/HTML đặc biệt nào) — `marked` đi qua token này như văn
 * bản thường, không hiểu/không đụng gì tới nó. Token có index đơn điệu và độc
 * nhất trong toàn bộ lượt render nên không bao giờ trùng nội dung thật.
 *
 * Lưu `source` (LaTeX gốc, chưa render) thay vì render KaTeX ngay ở bước
 * trích — `renderMarkdown` và `markdownToPlainText` cần 2 THỨ KHÁC NHAU từ
 * cùng 1 công thức: bản đầu cần HTML đã render đẹp, bản sau (dùng khi CHÉP
 * câu trả lời) chỉ cần hiện lại đúng cú pháp LaTeX gốc — lấy `textContent`
 * của HTML KaTeX cho việc chép sẽ ra 1 chuỗi ký tự rời rạc, sai thứ tự đọc
 * (KaTeX định vị từng ký tự bằng CSS tuyệt đối, DOM order không theo đúng
 * thứ tự đọc với công thức phức tạp như phân số/luỹ thừa lồng nhau). */
interface MathBlock {
  token: string;
  source: string;
  displayMode: boolean;
}

function renderMath(src: string, displayMode: boolean): string {
  // `throwOnError: false`: model có thể hallucinate cú pháp LaTeX sai (dấu
  // ngoặc lệch, lệnh không tồn tại...) — KaTeX sẽ tự vẽ ra thông báo lỗi màu
  // đỏ NGAY TẠI CHỖ thay vì throw exception làm hỏng luôn cả tin nhắn.
  // `trust: false` (mặc định): chặn các lệnh có thể nguy hiểm (\href,
  // \includegraphics...) — an toàn để chèn thẳng HTML output vào DOM mà
  // không cần DOMPurify xử lý lại (xem renderMarkdown bên dưới).
  return katex.renderToString(src, { throwOnError: false, displayMode, output: "html" });
}

/** Trích công thức LaTeX ra khỏi 1 đoạn text THƯỜNG (không phải code), thay
 * bằng token, trả về text đã thay + danh sách khối đã render.
 *
 * CỐ TÌNH KHÔNG hỗ trợ "$...$" (1 dấu $ đơn) cho công thức inline — app này
 * hay được hỏi về GIÁ CẢ ("$50", "$10/tháng"...), 1 dấu $ đơn rất dễ đụng độ
 * thật (VD "Giá là $50, còn phí ship $10" sẽ bị hiểu nhầm "50, còn phí ship "
 * là công thức toán nằm giữa 2 dấu $ đó). Chỉ nhận "$$...$$" (khối, ít khi
 * lẫn với tiền vì luôn tách dòng riêng) và "\(...\)"/"\[...\]" (dấu \ đứng
 * trước gần như chắc chắn là LaTeX, không phải cách viết tiền tệ thông
 * thường) — đã dặn lại AI dùng đúng quy ước này trong SYSTEM_PROMPT (ai.rs).
 */
function extractMath(text: string, blocks: MathBlock[]): string {
  let out = text;

  const push = (src: string, displayMode: boolean): string => {
    const token = `MDMATHTOKEN${blocks.length}ENDTOKEN`;
    blocks.push({ token, source: src.trim(), displayMode });
    return token;
  };

  // Khối (display) trước — "$$...$$" và "\[...\]" đều xuống dòng riêng.
  out = out.replace(/\$\$([\s\S]+?)\$\$/g, (_m, src: string) => push(src, true));
  out = out.replace(/\\\[([\s\S]+?)\\\]/g, (_m, src: string) => push(src, true));
  // Inline — "\(...\)".
  out = out.replace(/\\\(([\s\S]+?)\\\)/g, (_m, src: string) => push(src, false));

  return out;
}

/** "2*3*4" (phép nhân) bị `marked` hiểu nhầm thành in nghiêng ("2_3_4" với số
 * 3 in nghiêng) — CHỈ xảy ra khi dấu `*` nằm giữa 2 CHỮ SỐ, không đụng tới
 * in nghiêng thật (`*chữ*`) vì đó luôn có ít nhất 1 phía không phải chữ số.
 * Đổi hẳn sang dấu nhân "×" thật — vừa tránh bị hiểu nhầm, vừa rõ nghĩa hơn
 * hẳn so với "*" (dấu nhân toán học thật, không phải ký hiệu lập trình). */
function fixAsteriskMultiplication(text: string): string {
  return text.replace(/(\d)\s*\*\s*(?=\d)/g, "$1 × ");
}

/** Placeholder/thẻ dạng "<tên_biến>", ví dụ HTML thô "<div>" viết NGOÀI code
 * (không bọc trong dấu `code`) — trình duyệt hiểu đây là 1 thẻ HTML thật,
 * DOMPurify xoá thẻ không nằm trong danh sách cho phép, và vì không có chữ
 * nào NẰM GIỮA để giữ lại làm text, cả cụm biến mất khỏi màn hình hoàn toàn,
 * không có gì báo hiệu là đã mất chữ. Escape các cụm "trông giống thẻ HTML"
 * (giữa `<` và `>` không có khoảng trắng xuống dòng) thành `&lt;`/`&gt;` để
 * luôn hiện ra dưới dạng CHỮ, không bị nuốt — trừ autolink kiểu chuẩn markdown
 * "<https://...>"/"<mailto:...>" (giữ nguyên, đây là link thật, có ích). */
function escapeLooseTags(text: string): string {
  return text.replace(/<\/?[^\s<>]+(?:\s[^<>]*)?>/g, (match) => {
    if (/^<\/?(?:https?:\/\/|mailto:)/i.test(match)) return match;
    return match.replace(/</g, "&lt;").replace(/>/g, "&gt;");
  });
}

function preprocess(text: string): { text: string; blocks: MathBlock[] } {
  const blocks: MathBlock[] = [];
  const segments = splitCodeSegments(text).map((seg) => {
    if (seg.code) return seg.text; // code/inline-code: giữ nguyên 100%
    let t = extractMath(seg.text, blocks);
    t = escapeLooseTags(t);
    t = fixAsteriskMultiplication(t);
    return t;
  });
  return { text: segments.join(""), blocks };
}

export function renderMarkdown(text: string): string {
  const { text: preprocessed, blocks } = preprocess(text);
  const rawHtml = marked.parse(preprocessed, { async: false }) as string;
  // `ADD_TAGS`/`ADD_ATTR`: đảm bảo <button data-ts> (dùng cho mốc giờ video
  // bấm được, xem linkifyTimestamps bên dưới) LUÔN sống sót qua sanitize —
  // không dựa vào việc DOMPurify mặc định có cho phép "button"/"data-ts" hay
  // không (không nên đoán, khai báo rõ cho chắc).
  let clean = DOMPurify.sanitize(rawHtml, { ADD_TAGS: ["button"], ADD_ATTR: ["data-ts"] });
  // Chèn lại HTML công thức toán ĐÃ RENDER SẴN (KaTeX, an toàn — xem
  // renderMath) — làm SAU sanitize, không phải trước, vì output của KaTeX
  // chỉ gồm <span> + class/style/aria-hidden (đã tự kiểm tra), tự tin chèn
  // thẳng mà không cần DOMPurify xử lý lại; token thay thế nó là chuỗi chữ+số
  // thuần nên sanitize ở trên không hề đụng/làm hỏng token.
  for (const { token, source, displayMode } of blocks) {
    clean = clean.replaceAll(token, renderMath(source, displayMode));
  }
  return clean;
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
  // KHÔNG tin tưởng model theo đúng 100% định dạng đã dặn trong prompt —
  // thực tế đã gặp CẢ 2 kiểu lệch chuẩn trong cùng 1 buổi test:
  //   1. Bọc thêm backtick quanh mốc giờ (`[00:00]`) — theo đúng quy tắc #4
  //      trong SYSTEM_PROMPT ("dùng `code` cho giá trị kỹ thuật"), model tự
  //      áp luôn cho mốc giờ dù không phải ý đó. Backtick tạo ra 1 code-span
  //      markdown, <button> chèn vào TRONG đó bị hiện dạng chữ thô (code-span
  //      luôn escape nội dung), không render thành thẻ thật.
  //   2. Bỏ hẳn dấu ngoặc vuông (chỉ còn "00:00" trần) — không khớp regex chỉ
  //      nhận đúng dạng "[mm:ss]", nên chẳng có nút nào được tạo ra cả.
  // Giải pháp: CHUẨN HOÁ về đúng 1 dạng "[mm:ss]" trước (chấp nhận có/không
  // backtick, có/không ngoặc vuông), rồi mới chèn nút — không cần đoán thêm
  // model sẽ lệch chuẩn kiểu nào tiếp theo.
  const normalized = text.replace(/`?\[?(\d{1,2}):([0-5]\d)\]?`?/g, (_m, mm: string, ss: string) => `[${mm}:${ss}]`);
  return normalized.replace(/\[(\d{1,2}):([0-5]\d)\]/g, (match, mm: string, ss: string) => {
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
  // CỐ TÌNH không tái dùng renderMarkdown() ở đây — cần giữ lại token công
  // thức toán để thay bằng LATEX GỐC (dễ đọc), không phải lấy textContent từ
  // HTML KaTeX đã render (chữ số/ký hiệu bị rời rạc sai thứ tự đọc, xem giải
  // thích ở khai báo MathBlock).
  const { text: preprocessed, blocks } = preprocess(text);
  const rawHtml = marked.parse(preprocessed, { async: false }) as string;
  const cleanHtml = DOMPurify.sanitize(rawHtml, { ADD_TAGS: ["button"], ADD_ATTR: ["data-ts"] });
  const doc = new DOMParser().parseFromString(cleanHtml, "text/html");

  doc.querySelectorAll("br").forEach((el) => el.replaceWith("\n"));
  doc.querySelectorAll("li").forEach((el) => el.prepend("• "));
  doc
    .querySelectorAll("p, div, h1, h2, h3, h4, h5, h6, li, tr, blockquote, pre")
    .forEach((el) => el.append("\n"));

  let plain = doc.body.textContent ?? "";
  for (const { token, source } of blocks) {
    plain = plain.replaceAll(token, source);
  }
  return plain.replace(/\n{3,}/g, "\n\n").trim();
}
