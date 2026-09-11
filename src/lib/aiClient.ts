// Gọi AI (NVIDIA NIM / OpenAI / Anthropic / Gemini) — logic HTTP thực sự nằm
// ở Rust (src-tauri/src/ai.rs), gọi qua `invoke()`.
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
 * provider đang cấu hình, kiểu streaming. `onDelta` được gọi cho mỗi đoạn text
 * mới (nhận qua event "ai:delta" từ Rust); `onStatus` báo tiến trình để hiển
 * thị/debug. Trả về câu trả lời cuối cùng khi lệnh Rust hoàn tất. */
export async function askAIStream(
  history: ChatTurn[],
  settings: Settings,
  onDelta: (piece: string) => void,
  onStatus: (s: string) => void = () => {},
  /** Nút "Hỏi thêm về vùng này" (khoanh vùng AI chỉ tới) — [ymin,xmin,ymax,xmax]
   * thang 0-1000, chỉ Rust phía Gemini đọc field này (ask_ai_gemini), 3
   * provider còn lại tự bỏ qua field lạ (xem giải thích ở `ChatTurn` trên). */
  region?: [number, number, number, number] | null,
  /** Bật "Tra cứu web thật" (Google Search grounding, chỉ Gemini đọc field
   * này) — tính phí theo lượt Google tự quyết định search, nên để người dùng
   * TỰ BẬT từng lần hỏi (không mặc định bật, không "dính" qua các câu hỏi
   * tiếp theo), tránh đốt quota vô tình. */
  search?: boolean,
): Promise<string> {
  const windowLabel = getCurrentWindow().label;
  const model = currentModel(settings);

  let unlisten: UnlistenFn | undefined;
  try {
    unlisten = await listen<DeltaPayload>("ai:delta", (event) => {
      onDelta(event.payload.piece);
    });

    // KHÔNG nêu tên provider/model — người dùng phổ thông không cần biết
    // (và không nên phải biết) đang chạy AI nào phía sau, chỉ cần biết máy
    // đang xử lý.
    onStatus("Đang đợi");

    const common = { windowLabel, model, history, region: region ?? null, search: search ?? false };
    let answer: string;

    switch (settings.provider) {
      case "nvidia":
        answer = await invoke<string>("ask_ai_nvidia", {
          ...common,
          // `null` -> Rust không gửi tham số reasoning_effort lên API luôn
          // (không phải model NVIDIA nào cũng hỗ trợ khái niệm "reasoning").
          reasoningEffort: settings.nvidiaReasoningEnabled ? settings.nvidiaReasoningEffort || "none" : null,
        });
        break;
      case "openai":
        answer = await invoke<string>("ask_ai_openai", common);
        break;
      case "anthropic":
        answer = await invoke<string>("ask_ai_anthropic", common);
        break;
      case "gemini":
        answer = await invoke<string>("ask_ai_gemini", common);
        break;
      default: {
        const err = `Provider không hợp lệ: ${settings.provider}`;
        onStatus(err);
        throw new Error(err);
      }
    }

    onStatus("");
    return answer;
  } finally {
    unlisten?.();
  }
}
