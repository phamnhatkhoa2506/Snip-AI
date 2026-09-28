// Durable Object làm "trạm trung chuyển" gọi Gemini — CHỈ để GHIM CỨNG vị trí
// chạy, không giữ state gì (không cần storage/alarm).
//
// LÝ DO CẦN OBJECT NÀY (không gọi thẳng từ Worker chính): Gemini API chặn
// theo vị trí của BÊN GỌI — ở đây là Cloudflare, không phải máy người dùng.
// Worker THƯỜNG (kể cả bật "Smart Placement" ở wrangler.toml) chạy ở colo GẦN
// NGƯỜI DÙNG nhất, và Cloudflare có thể route sang colo nằm trong vùng Google
// chặn (lỗi thực tế đã gặp: "User location is not supported for the API
// use" dù người dùng đang ở vùng được hỗ trợ). Smart Placement chỉ là
// BEST-EFFORT — cần ~15 phút + đủ traffic để Cloudflare "học" ra vị trí tối
// ưu, không có gì đảm bảo, và có thể lệch lại nếu traffic thấp/đứt quãng.
//
// Durable Object thì NGƯỢC LẠI — `locationHint` truyền lúc lấy stub (xem
// index.ts) là CAM KẾT CỨNG của Cloudflare, không phải đoán theo traffic.
// Chọn "wnam" (Tây Bắc Mỹ) — gần cụm server Gemini API (đặt tại Mỹ) nhất,
// loại bỏ hẳn khả năng "trúng phải colo bị chặn" bất kể traffic thế nào.
import type { Env } from "./index";

/** Random hoá thứ tự mảng (Fisher-Yates) — dùng để chọn thứ tự thử các API
 * key, rải đều tải giữa các key thay vì luôn ưu tiên key đầu tiên. */
function shuffle<T>(arr: T[]): T[] {
  const a = [...arr];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/** Gọi Gemini, thử LẦN LƯỢT các key theo thứ tự đã random hoá — chỉ chuyển
 * sang key tiếp theo khi lỗi có khả năng do QUOTA/RATE LIMIT của riêng key đó
 * (429) hoặc lỗi tạm thời phía Google (5xx). Lỗi 4xx khác (VD 400 do body sai
 * định dạng, hoặc CHÍNH lỗi chặn vùng — không liên quan tới key nào) là lỗi
 * CHUNG cho mọi key — thử lại key khác vô ích, trả lỗi luôn.
 *
 * QUAN TRỌNG: chỉ đọc `resp.status` ở đây, CHƯA đụng vào `resp.body` — nhờ
 * vậy an toàn để "bỏ" response và thử key khác mà không làm hỏng stream (một
 * khi đã bắt đầu đọc/forward body thì không thể quay lại thử key khác nữa).
 */
async function callGeminiWithFailover(keys: string[], model: string, body: string): Promise<Response> {
  const upstream = `https://generativelanguage.googleapis.com/v1beta/models/${model}:streamGenerateContent?alt=sse`;
  const order = shuffle(keys);

  let lastResp: Response | null = null;
  for (const key of order) {
    const resp = await fetch(upstream, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-goog-api-key": key,
        accept: "text/event-stream",
      },
      body,
    });

    if (resp.ok) return resp;

    lastResp = resp;
    const retryable = resp.status === 429 || resp.status >= 500;
    if (!retryable) return resp; // lỗi không liên quan tới key -> trả luôn, không thử key khác
    // key này hết quota/rate-limit -> bỏ qua response body, thử key tiếp theo
  }

  // Hết key mà key nào cũng lỗi retryable -> trả về lỗi của lần thử cuối.
  return lastResp!;
}

export class GeminiProxy implements DurableObject {
  // Không cần constructor lưu `state`/`env` — object này KHÔNG đọc/ghi
  // storage gì cả, chỉ tồn tại để ghim vị trí chạy (xem giải thích ở đầu
  // file). Tham số vẫn khai báo đủ theo interface DurableObject.
  constructor(_state: DurableObjectState, _env: Env) {}

  /** Nhận `model` qua header (tránh phải parse lại JSON body — body forward
   * NGUYÊN VĂN từ client, không đụng vào), `keys` qua header (JSON đã stringify
   * sẵn từ phía gọi), body chính là request body gốc gửi thẳng cho Gemini. */
  async fetch(request: Request): Promise<Response> {
    const model = request.headers.get("x-gemini-model") ?? "";
    const keysHeader = request.headers.get("x-gemini-keys") ?? "[]";
    let keys: string[];
    try {
      keys = JSON.parse(keysHeader);
    } catch {
      keys = [];
    }
    if (!model || keys.length === 0) {
      return new Response(JSON.stringify({ error: "GeminiProxy: thiếu model hoặc keys" }), {
        status: 500,
        headers: { "content-type": "application/json" },
      });
    }
    const body = await request.text();
    return callGeminiWithFailover(keys, model, body);
  }
}
