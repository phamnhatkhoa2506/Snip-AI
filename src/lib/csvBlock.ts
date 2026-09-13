// Khối mã ```csv — khi người dùng NHỜ THẲNG "tạo/xuất file CSV" (không phải
// hỏi thường rồi tự bấm nút "Xuất file" trên bong bóng), AI trả lời bằng
// đúng 1 khối ```csv chứa NGUYÊN VĂN nội dung CSV (xem rule liên quan trong
// SYSTEM_PROMPT, ai.rs) — khối này quét DOM thêm nút "Tải CSV"/"Tải Excel"
// ngay dưới, KHÁC 3 loại khối kia (mermaid/plot/svg): KHÔNG thay hẳn nội
// dung hiển thị, chỉ thêm nút bên cạnh — người dùng vẫn xem/copy được văn
// bản CSV thô như bình thường, có ích khi muốn dán tay vào nơi khác.
//
// Cùng pattern quét DOM sau khi markdown.ts render xong (xem mermaid.ts).
import type { Action } from "svelte/action";
import { saveBytesAs } from "./exportImage";

/** Parser CSV tối giản nhưng ĐÚNG CHUẨN (RFC 4180) — xử lý được field có
 * dấu phẩy/xuống dòng/dấu ngoặc kép bên trong khi được bọc trong `"..."`
 * (dấu ngoặc kép nhân đôi `""` là 1 dấu ngoặc kép thật trong field). Cần viết
 * tay parser này (không dùng `.split(",")` đơn giản) vì AI thường tự ý bọc
 * ngoặc kép cho field có ký tự đặc biệt (dấu ngoặc, †...), `.split(",")` đơn
 * giản sẽ tách sai ngay khi gặp field bọc ngoặc kép có dấu phẩy bên trong. */
export function parseCsvText(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let inQuotes = false;
  let i = 0;
  const n = text.length;

  const pushField = () => {
    row.push(field);
    field = "";
  };
  const pushRow = () => {
    pushField();
    rows.push(row);
    row = [];
  };

  while (i < n) {
    const ch = text[i];
    if (inQuotes) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          field += '"';
          i += 2;
          continue;
        }
        inQuotes = false;
        i++;
        continue;
      }
      field += ch;
      i++;
      continue;
    }
    if (ch === '"') {
      inQuotes = true;
      i++;
      continue;
    }
    if (ch === ",") {
      pushField();
      i++;
      continue;
    }
    if (ch === "\r") {
      i++;
      continue; // bỏ qua, xử lý xuống dòng thật ở "\n"
    }
    if (ch === "\n") {
      pushRow();
      i++;
      continue;
    }
    field += ch;
    i++;
  }
  // Dòng cuối có thể không kết thúc bằng "\n" — vẫn phải đẩy nốt field/dòng
  // đang dang dở, trừ khi toàn bộ input đã rỗng ngay từ đầu.
  if (field !== "" || row.length > 0) pushRow();
  return rows.filter((r) => !(r.length === 1 && r[0] === ""));
}

async function renderCsvBlocks(container: HTMLElement): Promise<void> {
  const codeEls = Array.from(container.querySelectorAll<HTMLElement>("code.language-csv"));
  if (codeEls.length === 0) return;

  for (const codeEl of codeEls) {
    const pre = codeEl.closest("pre");
    if (!pre || pre.dataset.csvTried === "1") continue;
    pre.dataset.csvTried = "1";

    const rawCsv = codeEl.textContent ?? "";
    if (!rawCsv.trim()) continue;

    // Bọc <pre> trong 1 wrapper `relative` để đặt nút nổi góc trên-phải —
    // GIỮ NGUYÊN <pre> bên trong (không thay thế nội dung như mermaid/plot/
    // svg) vì văn bản CSV thô vẫn có ích để xem/copy tay.
    const wrapper = document.createElement("div");
    wrapper.style.cssText = "position:relative;";
    pre.replaceWith(wrapper);
    wrapper.appendChild(pre);

    const toolbar = document.createElement("div");
    toolbar.style.cssText = "position:absolute;top:6px;right:6px;display:flex;gap:4px;";
    wrapper.appendChild(toolbar);

    function makeBtn(label: string, title: string, onClick: () => void): void {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.title = title;
      btn.textContent = label;
      btn.style.cssText =
        "font-size:11px;font-weight:600;padding:3px 8px;border-radius:6px;" +
        "border:1px solid var(--color-border);background:var(--color-bg-elevated);" +
        "color:var(--color-text-muted);cursor:pointer;";
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        onClick();
      });
      toolbar.appendChild(btn);
    }

    makeBtn("⬇ CSV", "Tải file CSV", () => {
      // Nội dung ĐÃ LÀ CSV — lưu nguyên văn, chỉ thêm BOM để Excel đọc đúng
      // chữ có dấu tiếng Việt (cùng lý do đã áp dụng ở exportFile.ts).
      const bytes = new TextEncoder().encode("﻿" + rawCsv).buffer;
      saveBytesAs(bytes, "du-lieu.csv", [{ name: "CSV", extensions: ["csv"] }]).catch((err) =>
        console.warn("[snip-ai] Tải CSV thất bại:", err),
      );
    });

    makeBtn("⬇ Excel", "Tải file Excel (.xlsx)", async () => {
      try {
        const rows = parseCsvText(rawCsv);
        if (rows.length === 0) return;
        const ExcelJS = (await import("exceljs")).default;
        const workbook = new ExcelJS.Workbook();
        const sheet = workbook.addWorksheet("Dữ liệu");
        rows.forEach((r) => sheet.addRow(r));
        sheet.getRow(1).font = { bold: true };
        const buffer = await workbook.xlsx.writeBuffer();
        await saveBytesAs(buffer as ArrayBuffer, "du-lieu.xlsx", [{ name: "Excel", extensions: ["xlsx"] }]);
      } catch (err) {
        console.warn("[snip-ai] Tải Excel từ CSV thất bại:", err);
      }
    });
  }
}

/** Action gắn vào container `.markdown-body` — cùng cách dùng với
 * `mermaidBlocks`/`plotBlocks`/`svgFigureBlocks`/`chartBlocks`. */
export const csvBlocks: Action<HTMLElement, unknown> = (node) => {
  renderCsvBlocks(node);
  return {
    update() {
      renderCsvBlocks(node);
    },
  };
};
