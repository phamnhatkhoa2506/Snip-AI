// Xuất PNG từ biểu đồ/sơ đồ/hình đã vẽ (mermaid/plot/svg/chart) ra FILE THẬT
// trên đĩa — dùng CHUNG cho cả 4 loại khối đã có (mermaid.ts, plot.ts,
// svgFigure.ts, chart.ts), tránh viết lặp lại 4 lần cùng 1 logic "canvas ->
// PNG bytes -> lưu file". Mỗi loại chỉ khác nhau ở NGUỒN vẽ (SVG hay canvas),
// xem 2 hàm export bên dưới.
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";

/** Đổi `ArrayBuffer` sang base64 — Tauri command nhận string (JSON), không
 * nhận thẳng binary, nên mọi lệnh ghi file trong app đều đi qua base64 (cùng
 * quy ước đã dùng cho ảnh/video chụp màn hình, xem ai.rs/attachments.rs). */
function arrayBufferToBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  // `btoa` cần 1 chuỗi "binary string" (mỗi ký tự = đúng 1 byte) — ghép trực
  // tiếp qua String.fromCharCode. Ảnh PNG biểu đồ thường chỉ vài chục-vài
  // trăm KB nên ghép từng byte 1 không đáng lo về hiệu năng (khác trường hợp
  // file nhiều MB mới cần chia batch).
  let binary = "";
  for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
  return btoa(binary);
}

/** Mở hộp thoại "Lưu file" rồi ghi `bytes` ra đúng đường dẫn người dùng chọn.
 * Trả về `false` nếu người dùng bấm Huỷ (không phải lỗi) — gọi nơi khác tự
 * quyết có cần báo gì thêm hay im lặng bỏ qua. */
export async function saveBytesAs(
  bytes: ArrayBuffer,
  suggestedName: string,
  filters: { name: string; extensions: string[] }[],
): Promise<boolean> {
  const path = await save({ defaultPath: suggestedName, filters });
  if (!path) return false;
  await invoke("write_export_file", { path, dataB64: arrayBufferToBase64(bytes) });
  return true;
}

/** Xuất PNG từ 1 phần tử `<svg>` ĐÃ RENDER SẴN trong trang (mermaid/plot/svg
 * hình học đều dựng bằng SVG) — dùng canvas trung gian: vẽ SVG lên canvas rồi
 * `canvas.toBlob()` ra PNG thật. Nhân đôi độ phân giải (`scale = 2`) so với
 * kích thước hiển thị để ảnh xuất ra nét hơn khi dán vào tài liệu/trình
 * chiếu (phóng to không bị vỡ như chụp màn hình thường). */
export async function exportSvgAsPng(svgEl: SVGSVGElement, suggestedName: string): Promise<boolean> {
  const rect = svgEl.getBoundingClientRect();
  const scale = 2;
  const width = Math.max(1, Math.round((rect.width || 400) * scale));
  const height = Math.max(1, Math.round((rect.height || 300) * scale));

  // Nhân bản (không sửa trực tiếp phần tử đang hiển thị trên trang) — ép kích
  // thước THẬT trên bản sao trước khi serialize, để ảnh xuất ra đúng tỉ lệ
  // đã đo, không phụ thuộc CSS width/height% đang áp cho bản gốc.
  const clone = svgEl.cloneNode(true) as SVGSVGElement;
  clone.setAttribute("width", String(width));
  clone.setAttribute("height", String(height));
  const svgText = new XMLSerializer().serializeToString(clone);
  const svgDataUrl = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svgText)}`;

  const img = new Image();
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("Không tải được ảnh SVG để chuyển sang PNG"));
    img.src = svgDataUrl;
  });

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Không tạo được canvas để xuất ảnh");
  // Nền TRẮNG trước khi vẽ đè SVG lên — khớp đúng quy ước "biểu đồ luôn nền
  // trắng" đã áp dụng cho cả 4 loại khối (xem mermaid.ts/plot.ts/svgFigure.ts/
  // chart.ts) — nếu không, phần trong suốt của SVG xuất ra PNG sẽ bị đen khi
  // mở bằng trình xem ảnh không hỗ trợ trong suốt (VD dán vào 1 số slide).
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(img, 0, 0, width, height);

  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("Không tạo được ảnh PNG");
  const buf = await blob.arrayBuffer();
  return saveBytesAs(buf, suggestedName, [{ name: "Ảnh PNG", extensions: ["png"] }]);
}

/** Xuất PNG từ 1 chuỗi data URL đã có sẵn (VD `ECharts.getDataURL()` — tự vẽ
 * canvas nội bộ, đã trả sẵn base64 PNG, không cần dựng lại canvas thủ công
 * như hàm trên) — chỉ cần decode rồi lưu file. */
export async function exportDataUrlAsPng(dataUrl: string, suggestedName: string): Promise<boolean> {
  const res = await fetch(dataUrl);
  const buf = await res.arrayBuffer();
  return saveBytesAs(buf, suggestedName, [{ name: "Ảnh PNG", extensions: ["png"] }]);
}
