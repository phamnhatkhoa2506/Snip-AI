// Các prompt dựng sẵn cho chip gợi ý. Đây KHÔNG phải "chức năng" riêng biệt —
// mọi chip đều đi qua chung 1 đường gọi AI, chỉ khác nội dung câu hỏi.
//
// Lưu ý: đã có system prompt chung ở phía Rust (ai.rs) ép model trả lời gọn,
// có cấu trúc Markdown, không thêm lời dẫn/lời kết thừa. Các prompt dưới đây
// chỉ mô tả NHIỆM VỤ cụ thể, không cần lặp lại các quy tắc chung đó.

export const PROMPT_OCR =
  "Trích xuất toàn bộ văn bản có trong ảnh. Giữ nguyên thứ tự đọc, cách xuống dòng và " +
  "cấu trúc (tiêu đề, danh sách, bảng nếu có). Giữ nguyên ngôn ngữ gốc, không dịch. " +
  "Chỉ xuất phần văn bản trích xuất được — không thêm bất kỳ nhận xét, tiêu đề hay giải thích nào.";

export const PROMPT_TRANSLATE =
  "Dịch toàn bộ văn bản trong ảnh sang tiếng Việt tự nhiên, đúng văn phong gốc. " +
  "CHỈ xuất bản dịch — KHÔNG kèm văn bản gốc, KHÔNG kèm phiên âm, KHÔNG giải thích. " +
  "Giữ nguyên cấu trúc trình bày (đoạn, danh sách, bảng) của bản gốc. " +
  "Giữ nguyên không dịch: tên riêng, thuật ngữ kỹ thuật đã quen dùng tiếng Anh, đoạn mã, tên hàm/biến. " +
  "Nếu văn bản gốc vốn đã là tiếng Việt thì cứ trả về lại tiếng Việt.";

export const PROMPT_SUMMARIZE =
  "Tóm tắt nội dung chính trong ảnh theo cấu trúc:\n" +
  "- Dòng đầu: một câu tóm lược tổng thể (in đậm).\n" +
  "- Sau đó: các gạch đầu dòng cho từng ý chính, mỗi ý một dòng ngắn gọn.\n" +
  "- Nếu có số liệu/thông số quan trọng, giữ lại chính xác và làm nổi bật bằng `code`.";

export const PROMPT_EXPLAIN =
  "Giải thích nội dung trong ảnh cho người chưa có bối cảnh. Cấu trúc câu trả lời:\n" +
  "- **Đây là gì**: một câu xác định nội dung/loại nội dung đang thấy.\n" +
  "- **Nội dung chính**: các gạch đầu dòng giải thích những phần quan trọng.\n" +
  "- **Đáng chú ý**: (chỉ khi thực sự có) điểm bất thường, cảnh báo, lỗi, hoặc điều dễ hiểu nhầm.";

export const PROMPT_CODE =
  "Trong ảnh có mã nguồn hoặc thông báo lỗi. Hãy:\n" +
  "- **Mã/lỗi**: trích lại chính xác trong khối code, ghi rõ ngôn ngữ nếu nhận ra.\n" +
  "- **Ý nghĩa**: giải thích ngắn gọn đoạn mã làm gì, hoặc lỗi này nghĩa là gì.\n" +
  "- **Hướng xử lý**: (nếu là lỗi) các bước khắc phục cụ thể, theo thứ tự ưu tiên.";

export interface QuickPrompt {
  /** Định danh ổn định — dùng để lưu "hành động mặc định" trong Settings
   * (localStorage), KHÔNG dùng `label` vì label có thể đổi câu chữ sau này
   * mà không muốn phá cấu hình đã lưu của người dùng cũ. */
  id: string;
  /** tên icon trong Icon.svelte */
  icon: string;
  /** Chữ trên chip gợi ý */
  label: string;
  /** Chữ hiện trong bong bóng chat SAU KHI đã gửi — ngắn gọn, không phải
   * prompt đầy đủ. Prompt thật (dài, chi tiết) vẫn được gửi cho AI như bình
   * thường, chỉ là không hiện nguyên văn lên giao diện cho gọn. */
  chatLabel: string;
  /** Nội dung THẬT gửi cho AI, không hiển thị trực tiếp lên UI. */
  prompt: string;
}

export const QUICK_PROMPTS: QuickPrompt[] = [
  { id: "ocr", icon: "text", label: "Trích xuất chữ", chatLabel: "Trích xuất chữ trong ảnh", prompt: PROMPT_OCR },
  { id: "translate", icon: "languages", label: "Dịch", chatLabel: "Dịch sang tiếng Việt", prompt: PROMPT_TRANSLATE },
  { id: "summarize", icon: "list", label: "Tóm tắt", chatLabel: "Tóm tắt nội dung", prompt: PROMPT_SUMMARIZE },
  { id: "explain", icon: "lightbulb", label: "Giải thích", chatLabel: "Giải thích nội dung", prompt: PROMPT_EXPLAIN },
  { id: "code", icon: "code", label: "Mã / Lỗi", chatLabel: "Xem mã / lỗi trong ảnh", prompt: PROMPT_CODE },
];

// ⚠️ KHÔNG có gì đảm bảo mọi model reasoning trên NVIDIA NIM dùng chung 1 bộ
// giá trị này — chỉ xác nhận được đúng cho model kimi-k3 (theo code mẫu chính
// thức của NVIDIA). Model khác (DeepSeek-R1, QwQ...) có thể dùng tên field
// hoặc giá trị khác hẳn. Vì vậy KHÔNG dùng làm dropdown cố định — chỉ để làm
// gợi ý điền nhanh (chip), người dùng luôn có thể gõ giá trị khác. Giá trị
// viết THƯỜNG vì API mẫu của NVIDIA dùng "none"/"max" chữ thường.
export const REASONING_EFFORT_PRESETS = ["none", "low", "medium", "high", "max"] as const;
