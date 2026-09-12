// Các prompt dựng sẵn cho chip gợi ý. Đây KHÔNG phải "chức năng" riêng biệt —
// mọi chip đều đi qua chung 1 đường gọi AI, chỉ khác nội dung câu hỏi.
//
// Lưu ý: đã có system prompt chung ở phía Rust (ai.rs) ép model trả lời gọn,
// có cấu trúc Markdown, không thêm lời dẫn/lời kết thừa. Các prompt dưới đây
// chỉ mô tả NHIỆM VỤ cụ thể, không cần lặp lại các quy tắc chung đó.

export const PROMPT_OCR =
  "Trích xuất toàn bộ văn bản có trong ảnh. Giữ nguyên thứ tự đọc, cách xuống dòng và " +
  "cấu trúc (tiêu đề, danh sách, bảng nếu có). Giữ nguyên ngôn ngữ gốc, không dịch. " +
  "Chỉ xuất phần văn bản trích xuất được — không thêm bất kỳ nhận xét, tiêu đề hay giải thích nào. " +
  "CHỈ bọc trong khối code (```) phần THỰC SỰ là mã nguồn/lệnh dòng lệnh/log kỹ thuật; " +
  "văn bản thường (câu, đoạn văn, danh sách, bảng, phụ đề...) thì giữ nguyên dạng văn bản " +
  "thường, KHÔNG bọc code — kể cả khi ảnh chụp từ terminal/trình soạn thảo code.";

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

// ── "Vẽ sơ đồ" — chế độ TƯỜNG MINH (người dùng chủ động bấm/bật), khác với
// việc AI TỰ QUYẾT định vẽ hay không theo ngữ cảnh (xem SYSTEM_PROMPT rule #8
// trong ai.rs — vẫn hoạt động song song, độc lập với 2 hằng số dưới đây).
// 2 cách dùng:
//  1. Chip "Vẽ sơ đồ" trong bộ QUICK_PROMPTS/VIDEO_PROMPTS — 1 chạm, dùng
//     PROMPT_DIAGRAM/PROMPT_VIDEO_DIAGRAM có sẵn, không cần gõ gì thêm.
//  2. Nút bật/tắt "chế độ vẽ sơ đồ" cạnh ô nhập (result/+page.svelte) — BẮT
//     BUỘC AI vẽ cho ĐÚNG câu hỏi đang gõ, và người dùng có thể MÔ TẢ THÊM
//     cách vẽ ngay trong câu hỏi đó (VD "vẽ dạng sequence diagram", "chỉ lấy
//     5 bước chính") — DIAGRAM_MODE_SUFFIX chỉ ép "phải vẽ", còn "vẽ thế nào"
//     nằm nguyên trong câu người dùng tự gõ, không cần thêm ô nhập riêng.
export const PROMPT_DIAGRAM =
  "Vẽ 1 sơ đồ trực quan minh hoạ nội dung chính trong ảnh — tự chọn loại sơ đồ phù hợp nhất " +
  "(lưu đồ quy trình, sơ đồ tư duy, sơ đồ quan hệ, sequence, cấu trúc phân cấp...). " +
  "Dùng ĐÚNG 1 khối mã ```mermaid với cú pháp Mermaid hợp lệ. " +
  "Có thể kèm thêm vài dòng giải thích ngắn gọn bên cạnh sơ đồ, không bắt buộc.";

export const PROMPT_VIDEO_DIAGRAM =
  "Vẽ 1 sơ đồ trực quan minh hoạ các bước/luồng diễn ra trong video — tự chọn loại sơ đồ phù " +
  "hợp nhất (lưu đồ các bước theo thứ tự, sequence, sơ đồ tư duy tổng hợp...). " +
  "Dùng ĐÚNG 1 khối mã ```mermaid với cú pháp Mermaid hợp lệ. " +
  "Có thể kèm thêm vài dòng giải thích ngắn gọn bên cạnh sơ đồ, không bắt buộc.";

/** Nối vào CUỐI câu hỏi thật của người dùng khi "chế độ vẽ sơ đồ" đang BẬT —
 * ép AI PHẢI vẽ (khác PROMPT_DIAGRAM ở trên: đây chỉ là 1 câu ra lệnh ngắn
 * thêm vào SAU nội dung người dùng tự gõ, không thay hẳn nội dung câu hỏi). */
export const DIAGRAM_MODE_SUFFIX =
  "\n\n(Chế độ vẽ sơ đồ đang BẬT: BẮT BUỘC vẽ 1 sơ đồ Mermaid minh hoạ, theo đúng mô tả ở trên " +
  "nếu có — tự chọn loại sơ đồ phù hợp nhất, dùng khối mã ```mermaid.)";

// ── Prompt cho phiên VIDEO ────────────────────────────────────────────────
// Tách riêng khỏi bộ prompt ảnh ở trên vì 2 lý do, không phải chỉ để đổi chữ
// "ảnh" thành "video":
//
//  1. Gemini LẤY MẪU video ở khoảng 1 khung/giây, không xem từng khung. Chữ
//     chỉ hiện thoáng qua giữa 2 mốc lấy mẫu sẽ bị bỏ sót MÀ KHÔNG BÁO LỖI —
//     nguy hiểm hơn hẳn so với ảnh (ảnh mờ thì người dùng tự thấy). Nên các
//     prompt dưới đây ép model nói rõ khi nội dung trôi quá nhanh.
//  2. Video có TRỤC THỜI GIAN. "Trích xuất chữ" trên ảnh là câu hỏi rõ nghĩa,
//     nhưng trên video thì mơ hồ (gộp hết mọi chữ từng hiện? theo thứ tự thời
//     gian? chỉ khung cuối?) — không nói rõ thì mỗi lần model tự chọn một kiểu.
//
// Đổi lại, chip "Mã / Lỗi" không có bản video: đọc code trôi qua trên video
// vốn không đáng tin, và ca dùng thật khi quay màn hình là "quay lại thao tác
// gây lỗi rồi hỏi", nên thay bằng chip "Thao tác" mô tả các bước đã làm.

export const PROMPT_VIDEO_OCR =
  "Trích xuất văn bản xuất hiện trong video, sắp theo thứ tự thời gian.\n" +
  "- Mỗi mốc nội dung thay đổi thì xuống một mục mới, mở đầu bằng dấu thời gian dạng [mm:ss] (viết trần, KHÔNG bọc trong dấu backtick/code).\n" +
  "- Giữ nguyên ngôn ngữ gốc, không dịch. Không lặp lại nguyên khối văn bản không đổi giữa các mốc.\n" +
  "- Nếu có đoạn chữ trôi quá nhanh hoặc quá mờ để đọc chắc chắn, ghi rõ điều đó ở mốc tương ứng thay vì đoán.";

export const PROMPT_VIDEO_TRANSLATE =
  "Dịch văn bản xuất hiện trong video sang tiếng Việt tự nhiên, đúng văn phong gốc, theo thứ tự thời gian.\n" +
  "- Mỗi mốc nội dung thay đổi thì xuống một mục mới, mở đầu bằng dấu thời gian dạng [mm:ss] (viết trần, KHÔNG bọc trong dấu backtick/code).\n" +
  "- CHỈ xuất bản dịch — KHÔNG kèm văn bản gốc, KHÔNG kèm phiên âm, KHÔNG giải thích.\n" +
  "- Giữ nguyên không dịch: tên riêng, thuật ngữ kỹ thuật đã quen dùng tiếng Anh, đoạn mã, tên hàm/biến.\n" +
  "- Nếu văn bản gốc vốn đã là tiếng Việt thì cứ trả về lại tiếng Việt.";

export const PROMPT_VIDEO_SUMMARIZE =
  "Tóm tắt những gì diễn ra trong video theo cấu trúc:\n" +
  "- Dòng đầu: một câu tóm lược tổng thể (in đậm) — video này cho thấy chuyện gì.\n" +
  "- Sau đó: các gạch đầu dòng theo trình tự thời gian, mỗi dòng mở đầu bằng [mm:ss] (viết trần, KHÔNG bọc trong dấu backtick/code).\n" +
  "- Nếu có số liệu/thông số quan trọng, giữ lại chính xác và làm nổi bật bằng `code`.";

export const PROMPT_VIDEO_EXPLAIN =
  "Giải thích nội dung video cho người chưa có bối cảnh. Cấu trúc câu trả lời:\n" +
  "- **Đây là gì**: một câu xác định video đang cho thấy nội dung/màn hình gì.\n" +
  "- **Diễn biến**: các gạch đầu dòng theo trình tự thời gian, mỗi dòng mở đầu bằng [mm:ss] (viết trần, KHÔNG bọc trong dấu backtick/code).\n" +
  "- **Đáng chú ý**: (chỉ khi thực sự có) điểm bất thường, cảnh báo, lỗi, hoặc điều dễ hiểu nhầm.";

export const PROMPT_VIDEO_STEPS =
  "Video này ghi lại thao tác trên màn hình. Hãy mô tả lại các bước đã làm:\n" +
  "- Đánh số từng bước theo thứ tự, mỗi bước mở đầu bằng dấu thời gian [mm:ss] (viết trần, KHÔNG bọc trong dấu backtick/code).\n" +
  "- Mỗi bước ghi rõ: thao tác gì, lên phần tử nào (tên nút/menu/ô nhập chính xác nếu đọc được).\n" +
  "- **Kết quả**: màn hình cuối cùng cho thấy điều gì.\n" +
  "- **Trục trặc**: (chỉ khi thực sự có) thông báo lỗi, thao tác bị treo, hoặc bước có vẻ làm sai.";

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
  { id: "diagram", icon: "flowchart", label: "Vẽ sơ đồ", chatLabel: "Vẽ sơ đồ minh hoạ", prompt: PROMPT_DIAGRAM },
];

/** Chip gợi ý cho phiên QUAY VIDEO. `id` cố tình dùng chung với bộ ảnh
 * (`ocr`/`translate`/…) để cấu hình nào lưu theo id vẫn khớp giữa 2 chế độ. */
export const VIDEO_PROMPTS: QuickPrompt[] = [
  { id: "ocr", icon: "text", label: "Trích xuất chữ", chatLabel: "Trích xuất chữ trong video", prompt: PROMPT_VIDEO_OCR },
  { id: "translate", icon: "languages", label: "Dịch", chatLabel: "Dịch sang tiếng Việt", prompt: PROMPT_VIDEO_TRANSLATE },
  { id: "summarize", icon: "list", label: "Tóm tắt", chatLabel: "Tóm tắt video", prompt: PROMPT_VIDEO_SUMMARIZE },
  { id: "explain", icon: "lightbulb", label: "Giải thích", chatLabel: "Giải thích nội dung video", prompt: PROMPT_VIDEO_EXPLAIN },
  { id: "steps", icon: "mouse-pointer", label: "Thao tác", chatLabel: "Mô tả các bước đã làm", prompt: PROMPT_VIDEO_STEPS },
  { id: "diagram", icon: "flowchart", label: "Vẽ sơ đồ", chatLabel: "Vẽ sơ đồ minh hoạ", prompt: PROMPT_VIDEO_DIAGRAM },
];
