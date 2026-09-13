// Xuất câu trả lời AI ra FILE THẬT (CSV/Excel/Word/PDF) — khác hẳn nút "Chép"
// đã có (chỉ đưa vào clipboard). Phục vụ đúng nhu cầu thực tế: người dùng
// chụp 1 bảng số liệu/văn bản dài, muốn có ngay file Excel/Word thay vì phải
// gõ lại tay.
//
// NGUYÊN TẮC (giống hệt cách đã làm với mermaid/plot/svg/chart): KHÔNG để AI
// tự "tạo file" — AI chỉ trả lời bằng TEXT/Markdown như bình thường, việc
// ĐÓNG GÓI thành file nhị phân thật (xlsx/docx) do CODE xử lý, không phụ
// thuộc AI tự sinh đúng định dạng nhị phân phức tạp (bất khả thi qua text).
import { lexer, type Tokens } from "marked";
import { markdownToPlainText } from "./markdown";
// Dùng chung 1 hàm "lưu bytes ra file" với exportImage.ts (PNG biểu đồ) —
// tránh viết lặp lại cùng 1 logic gọi hộp thoại lưu file + ghi qua
// `write_export_file` ở 2 nơi.
import { saveBytesAs } from "./exportImage";

// ── Trích bảng Markdown ─────────────────────────────────────────────────────

export interface TableData {
  header: string[];
  rows: string[][];
}

/** Bỏ cú pháp Markdown INLINE cơ bản (`**đậm**`, `_nghiêng_`, `` `code` ``)
 * khỏi TỪNG Ô của bảng — dữ liệu xuất ra Excel/CSV nên là chữ SẠCH, không
 * dính ký tự cú pháp thô. Không dùng `markdownToPlainText` (dựng cho cả 1
 * đoạn dài, tự chèn xuống dòng ở mỗi thẻ block) — 1 ô bảng chỉ cần bóc NHANH
 * vài ký tự markdown đơn giản, không cần bộ máy render đầy đủ. */
function stripInlineMarkdown(text: string): string {
  return text
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/__(.+?)__/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*(.+?)\*/g, "$1")
    .trim();
}

/** Tìm bảng Markdown ĐẦU TIÊN trong câu trả lời (dùng `marked.lexer` có sẵn
 * — parser GFM table đã được kiểm chứng qua toàn bộ pipeline render hiện có,
 * đáng tin hơn tự viết regex riêng). Chỉ lấy bảng ĐẦU TIÊN — câu trả lời có
 * NHIỀU bảng là trường hợp hiếm, giữ đơn giản cho bản đầu của tính năng. */
export function extractFirstTable(markdown: string): TableData | null {
  const tokens = lexer(markdown);
  const tableToken = tokens.find((t): t is Tokens.Table => t.type === "table");
  if (!tableToken) return null;
  return {
    header: tableToken.header.map((cell) => stripInlineMarkdown(cell.text)),
    rows: tableToken.rows.map((row) => row.map((cell) => stripInlineMarkdown(cell.text))),
  };
}

// ── CSV ──────────────────────────────────────────────────────────────────────

function csvEscapeCell(cell: string): string {
  // Bọc trong ngoặc kép nếu có dấu phẩy/xuống dòng/ngoặc kép — chuẩn CSV
  // (RFC 4180); ngoặc kép bên trong nhân đôi thành `""`.
  if (/[",\n]/.test(cell)) return `"${cell.replace(/"/g, '""')}"`;
  return cell;
}

export async function exportTableAsCsv(table: TableData, suggestedName: string): Promise<boolean> {
  const lines = [table.header, ...table.rows].map((row) => row.map(csvEscapeCell).join(","));
  // BOM (﻿) ở đầu file — THIẾU dòng này Excel sẽ đọc sai chữ có dấu
  // tiếng Việt (hiện thành ký tự lạ) dù file vẫn là UTF-8 hợp lệ. Lỗi rất
  // phổ biến khi mở CSV UTF-8 thuần bằng Excel trên Windows.
  const csvText = "﻿" + lines.join("\r\n");
  const bytes = new TextEncoder().encode(csvText).buffer;
  return saveBytesAs(bytes, suggestedName, [{ name: "CSV", extensions: ["csv"] }]);
}

// ── Excel (.xlsx thật) ───────────────────────────────────────────────────────

export async function exportTableAsExcel(table: TableData, suggestedName: string): Promise<boolean> {
  // Dynamic import — exceljs khá nặng, chỉ tải khi người dùng thực sự bấm
  // xuất Excel, không phải mọi lần mở cửa sổ "Kết quả AI".
  const ExcelJS = (await import("exceljs")).default;
  const workbook = new ExcelJS.Workbook();
  const sheet = workbook.addWorksheet("Dữ liệu");
  sheet.addRow(table.header);
  for (const row of table.rows) sheet.addRow(row);
  // In đậm dòng tiêu đề + tự co giãn độ rộng cột theo nội dung dài nhất —
  // không bắt buộc nhưng file mở lên đỡ phải tự chỉnh tay ngay từ đầu.
  sheet.getRow(1).font = { bold: true };
  sheet.columns.forEach((col) => {
    let maxLen = 10;
    col.eachCell?.({ includeEmpty: true }, (cell) => {
      maxLen = Math.max(maxLen, String(cell.value ?? "").length);
    });
    col.width = Math.min(60, maxLen + 2);
  });
  const buffer = await workbook.xlsx.writeBuffer();
  return saveBytesAs(buffer as ArrayBuffer, suggestedName, [{ name: "Excel", extensions: ["xlsx"] }]);
}

// ── Word (.docx thật) — GIỮ ĐỊNH DẠNG (heading/đậm/bảng/danh sách) ──────────
//
// LỖI THỰC TẾ đã gặp ở bản đầu: xuất qua `markdownToPlainText` rồi chẻ theo
// "\n" thành các đoạn chữ thường — mất sạch heading/in đậm/bảng/danh sách,
// file Word ra chỉ là 1 khối chữ phẳng. Sửa bằng cách đi qua CHÍNH `marked
// .lexer()` (đã dùng để trích bảng ở trên) walk từng loại token Markdown rồi
// map sang ĐÚNG cấu trúc `docx` tương ứng (Heading1-6, TextRun đậm/nghiêng,
// Table/TableRow/TableCell thật, đoạn có `bullet` cho danh sách) — thay vì
// làm phẳng hết thành text, giữ được cấu trúc y hệt bản Markdown gốc.

/** Bóc các token INLINE (chữ thường, chữ đậm, chữ nghiêng, code) thành mảng
 * `TextRun` — đệ quy vì Markdown cho phép lồng nhau (VD đậm+nghiêng cùng lúc). */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function inlineTokensToRuns(docxMod: any, tokens: any[] | undefined, style: { bold?: boolean; italics?: boolean } = {}): any[] {
  if (!tokens) return [];
  const { TextRun } = docxMod;
  const runs: any[] = [];
  for (const t of tokens) {
    switch (t.type) {
      case "strong":
        runs.push(...inlineTokensToRuns(docxMod, t.tokens, { ...style, bold: true }));
        break;
      case "em":
        runs.push(...inlineTokensToRuns(docxMod, t.tokens, { ...style, italics: true }));
        break;
      case "codespan":
        runs.push(new TextRun({ text: t.text, font: "Cascadia Code", size: 22, ...style }));
        break;
      case "link":
      case "del":
        runs.push(...inlineTokensToRuns(docxMod, t.tokens, style));
        break;
      case "br":
        runs.push(new TextRun({ text: "", break: 1 }));
        break;
      default:
        runs.push(new TextRun({ text: t.text ?? t.raw ?? "", font: "Calibri", size: 24, ...style }));
    }
  }
  return runs;
}

/** Lấy đúng mảng token inline của 1 dòng list item — marked bọc thêm 1 lớp
 * token "text" trung gian bên trong `item.tokens`, phải bóc thêm 1 tầng nữa
 * mới ra tới token inline thật (khác `paragraph`/`heading` vốn có sẵn
 * `.tokens` ở tầng ngoài cùng). Bỏ qua sub-list lồng bên trong (item con của
 * item) — giữ đơn giản cho bản đầu, item con hiện thành text thường không
 * thụt lề thay vì mất hẳn. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function listItemInlineTokens(item: any): any[] {
  const first = item.tokens?.[0];
  if (first?.type === "text" && first.tokens) return first.tokens;
  return (item.tokens ?? []).filter((t: any) => t.type !== "list");
}

const HEADING_BY_LEVEL: Record<number, string> = {
  1: "Heading1",
  2: "Heading2",
  3: "Heading3",
  4: "Heading4",
  5: "Heading5",
  6: "Heading6",
};

/** Duyệt token cấp KHỐI (block) của `marked.lexer()`, trả về danh sách phần
 * tử `docx` (Paragraph/Table) tương ứng — đây là phần "dịch cấu trúc" chính
 * của cả tính năng xuất Word. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function blockTokensToDocx(docxMod: any, tokens: any[]): any[] {
  const { Paragraph, Table, TableRow, TableCell, WidthType } = docxMod;
  const out: any[] = [];

  for (const t of tokens) {
    switch (t.type) {
      case "heading":
        out.push(new Paragraph({ heading: HEADING_BY_LEVEL[t.depth] ?? "Heading6", children: inlineTokensToRuns(docxMod, t.tokens) }));
        break;
      case "paragraph":
        out.push(new Paragraph({ spacing: { after: 120 }, children: inlineTokensToRuns(docxMod, t.tokens) }));
        break;
      case "blockquote":
        // Mỗi đoạn con trong blockquote -> 1 Paragraph nghiêng, thụt lề trái —
        // giả lập dải màu bên trái của Markdown (docx không có "border-left"
        // đơn giản cho Paragraph thường, thụt lề là đủ dùng cho bản đầu).
        for (const inner of t.tokens ?? []) {
          if (inner.type !== "paragraph" && inner.type !== "text") continue;
          out.push(
            new Paragraph({
              indent: { left: 480 },
              children: inlineTokensToRuns(docxMod, inner.tokens, { italics: true }),
            }),
          );
        }
        break;
      case "list": {
        let idx = 0;
        for (const item of t.items ?? []) {
          const prefixRun = t.ordered
            ? new (docxMod.TextRun)({ text: `${(t.start || 1) + idx}. `, font: "Calibri", size: 24 })
            : null;
          out.push(
            new Paragraph({
              bullet: t.ordered ? undefined : { level: 0 },
              children: prefixRun ? [prefixRun, ...listItemInlineTokens(item).length ? inlineTokensToRuns(docxMod, listItemInlineTokens(item)) : []] : inlineTokensToRuns(docxMod, listItemInlineTokens(item)),
            }),
          );
          idx++;
        }
        break;
      }
      case "code":
        out.push(
          new Paragraph({
            shading: { fill: "F0F0F0" },
            children: [new (docxMod.TextRun)({ text: t.text, font: "Cascadia Code", size: 20 })],
          }),
        );
        break;
      case "table": {
        const headerRow = new TableRow({
          tableHeader: true,
          children: t.header.map(
            (cell: any) =>
              new TableCell({
                shading: { fill: "E5E5E5" },
                children: [new Paragraph({ children: inlineTokensToRuns(docxMod, cell.tokens, { bold: true }) })],
              }),
          ),
        });
        const bodyRows = t.rows.map(
          (row: any[]) =>
            new TableRow({
              children: row.map(
                (cell) => new TableCell({ children: [new Paragraph({ children: inlineTokensToRuns(docxMod, cell.tokens) })] }),
              ),
            }),
        );
        out.push(new Table({ width: { size: 100, type: WidthType.PERCENTAGE }, rows: [headerRow, ...bodyRows] }));
        // 1 đoạn trống ngay sau bảng — Word thường dồn đoạn kế tiếp sát ngay
        // mép dưới bảng nếu không có gì phân cách, trông rất chật.
        out.push(new Paragraph(""));
        break;
      }
      case "hr":
        out.push(new Paragraph({ border: { bottom: { color: "999999", space: 1, style: "single", size: 6 } } }));
        break;
      case "space":
        break;
      default:
        if (t.text) out.push(new Paragraph({ children: inlineTokensToRuns(docxMod, t.tokens ?? [{ type: "text", text: t.text }]) }));
    }
  }
  return out;
}

/** Xuất câu trả lời (Markdown GỐC, chưa qua `markdownToPlainText`) thành
 * file Word THẬT, GIỮ ĐỊNH DẠNG — heading thành Heading Word thật, **đậm**
 * thành chữ đậm thật, bảng thành bảng Word thật (không phải ký tự `|` thô),
 * danh sách thành bullet/số thứ tự thật. */
export async function exportMarkdownAsDocx(markdown: string, suggestedName: string): Promise<boolean> {
  // Dynamic import — cùng lý do với exceljs ở trên.
  const docxMod = await import("docx");
  const { Document, Packer, Paragraph } = docxMod;
  const tokens = lexer(markdown);
  const children = blockTokensToDocx(docxMod, tokens);
  const doc = new Document({
    sections: [{ children: children.length > 0 ? children : [new Paragraph("")] }],
  });
  // `toBlob` (không phải `toBuffer`) — tránh phụ thuộc `Buffer` (kiểu Node,
  // không có sẵn trong môi trường trình duyệt/WebView2 nếu không polyfill).
  const blob = await Packer.toBlob(doc);
  const buffer = await blob.arrayBuffer();
  return saveBytesAs(buffer, suggestedName, [{ name: "Word", extensions: ["docx"] }]);
}

// ── PDF (in trực tiếp, KHÔNG qua thư viện dựng PDF riêng) ───────────────────
//
// CỐ TÌNH không dùng thư viện kiểu `jsPDF` — jsPDF chỉ có sẵn vài font Latin
// cơ bản (Helvetica...), KHÔNG hỗ trợ dấu tiếng Việt (ệ, ư, ơ...) trừ khi tự
// nhúng thêm 1 file font TTF riêng (phức tạp, dễ sai, tốn thêm dung lượng cài
// đặt). Thay vào đó: mở 1 khung ẩn (`<iframe>`), chèn ĐÚNG HTML đã render sẵn
// (cùng font hệ thống app đang dùng — vốn đã hiển thị tiếng Việt đúng ngay
// trên màn hình), rồi gọi lệnh in của trình duyệt/OS (`window.print()`).
// WebView2 hỗ trợ "Lưu dưới dạng PDF" ngay trong hộp thoại in hiện ra — không
// cần thêm bước "chọn nơi lưu" riêng như CSV/Excel/Word (do bản chất hộp
// thoại in của OS, không phải Tauri dialog).
export function printHtmlAsPdf(bodyHtml: string, title: string): void {
  const iframe = document.createElement("iframe");
  iframe.style.cssText = "position:fixed;top:-10000px;left:-10000px;width:0;height:0;border:0;";
  document.body.appendChild(iframe);

  const doc = iframe.contentDocument;
  if (!doc) {
    iframe.remove();
    return;
  }
  doc.open();
  doc.write(`<!doctype html><html><head><meta charset="utf-8"><title>${title}</title>
    <style>
      body { font-family: "Segoe UI", Arial, sans-serif; font-size: 13px; line-height: 1.6; color: #111; padding: 24px; }
      h1, h2, h3, h4 { color: #111; }
      table { border-collapse: collapse; width: 100%; }
      td, th { border: 1px solid #999; padding: 6px 8px; }
      pre { background: #f3f3f3; padding: 10px; border-radius: 6px; overflow-x: auto; white-space: pre-wrap; }
      code { background: #f3f3f3; padding: 1px 4px; border-radius: 4px; }
    </style>
  </head><body>${bodyHtml}</body></html>`);
  doc.close();

  // Đợi 1 nhịp cho iframe load xong nội dung + font trước khi in — gọi print()
  // ngay lúc vừa `write()` xong có thể in ra trang trắng (chưa kịp layout).
  iframe.onload = () => {
    iframe.contentWindow?.focus();
    iframe.contentWindow?.print();
    // Dọn iframe sau khi hộp thoại in đã mở (không cần đợi người dùng in
    // xong/huỷ — hộp thoại in của OS độc lập với vòng đời iframe).
    setTimeout(() => iframe.remove(), 1000);
  };
}

export { markdownToPlainText };
