// Gọi AI (Gemini) — logic HTTP thực sự nằm ở Rust (src-tauri/src/ai.rs), gọi
// qua `invoke()`.
//
// Hai lý do quan trọng khiến phần này KHÔNG gọi HTTP trực tiếp từ JS:
// 1. BẢO MẬT: API key nằm trong OS keychain và chỉ Rust đọc được — key không
//    bao giờ xuất hiện trong bộ nhớ/JS của webview.
// 2. ỔN ĐỊNH: ảnh chụp đã nằm sẵn trong AppState ở Rust. Nếu gọi fetch từ JS,
//    ảnh (~30-40KB base64) phải đi qua IPC; trong `tauri dev`, IPC fallback
//    sang kênh `postMessage` có giới hạn kích thước và từng làm request treo
//    vô thời hạn. Gọi thẳng từ Rust tránh hẳn việc này.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { currentModel, type Settings } from "./settings";

export type ChatRole = "user" | "assistant";
export interface ChatTurn {
  role: ChatRole;
  /** Nội dung THẬT gửi cho AI (Rust chỉ đọc field này, `displayLabel` bị bỏ
   * qua tự động — Tauri/serde không lỗi khi frontend gửi thêm field lạ). */
  content: string;
  /** Chữ ngắn gọn hiện thay cho `content` trong bong bóng chat (VD chọn từ
   * chip gợi ý "Trích xuất chữ" thay vì cả đoạn prompt dài). Không set thì UI
   * hiện thẳng `content`. */
  displayLabel?: string;
}

interface DeltaPayload {
  piece: string;
}

/** Gửi ảnh (đã có sẵn trong AppState phía Rust) + toàn bộ hội thoại tới
 * Gemini, kiểu streaming. `onDelta` được gọi cho mỗi đoạn text mới (nhận qua
 * event "ai:delta" từ Rust); `onStatus` báo tiến trình để hiển thị/debug.
 * Trả về câu trả lời cuối cùng khi lệnh Rust hoàn tất. */
export async function askAIStream(
  history: ChatTurn[],
  settings: Settings,
  onDelta: (piece: string) => void,
  onStatus: (s: string) => void = () => {},
  /** Nút "Hỏi thêm về vùng này" (khoanh vùng AI chỉ tới) —
   * [ymin,xmin,ymax,xmax] thang 0-1000 (xem ask_ai_gemini trong ai.rs). */
  region?: [number, number, number, number] | null,
  /** Bật "Tra cứu web thật" (Google Search grounding) — tính phí theo lượt
   * Google tự quyết định search, nên để người dùng TỰ BẬT từng lần hỏi
   * (không mặc định bật, không "dính" qua các câu hỏi tiếp theo), tránh đốt
   * quota vô tình. */
  search?: boolean,
): Promise<string> {
  const windowLabel = getCurrentWindow().label;
  const model = currentModel(settings);

  let unlisten: UnlistenFn | undefined;
  try {
    unlisten = await listen<DeltaPayload>("ai:delta", (event) => {
      onDelta(event.payload.piece);
    });

    // KHÔNG nêu tên model — người dùng phổ thông không cần biết (và không
    // nên phải biết) đang chạy model nào phía sau, chỉ cần biết máy đang xử lý.
    onStatus("Đang đợi");

    const answer = await invoke<string>("ask_ai_gemini", {
      windowLabel,
      model,
      history,
      region: region ?? null,
      search: search ?? false,
    });

    onStatus("");
    return answer;
  } finally {
    unlisten?.();
  }
}

export interface VocabDiagramTerm {
  term: string;
  translation: string;
  relation: string;
  example: string;
}
export interface VocabDiagramData {
  mainTerm: string;
  translation: string;
  related: VocabDiagramTerm[];
}

/** "Sơ đồ từ vựng" — KHÁC HẲN askAIStream: không streaming (JSON ép cấu trúc
 * không có gì để "hiện dần"). */
export async function askAIDiagram(model: string): Promise<VocabDiagramData> {
  const windowLabel = getCurrentWindow().label;
  return invoke<VocabDiagramData>("ask_ai_diagram", { windowLabel, model });
}
