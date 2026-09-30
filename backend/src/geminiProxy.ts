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

/** Upload 1 file qua Gemini File API — giao thức "resumable upload" 2 bước
 * của Google: (1) "start" báo trước kích thước/mime, Google trả về 1 URL
 * upload riêng cho phiên đó (header `X-Goog-Upload-URL`); (2) "upload,
 * finalize" gửi thẳng bytes tới URL đó, nhận lại `file.uri` — dùng lại được
 * trong request `generateContent` (field `file_data`) trong ~48h, KHÔNG cần
 * gửi lại bytes mỗi lần hỏi tiếp trong cùng phiên (xem attachments.rs phía
 * Rust — nơi cache lại `file_uri` này).
 *
 * Cùng kiểu failover với `callGeminiWithFailover` ở trên — thử LẦN LƯỢT các
 * key, chỉ chuyển key khác khi lỗi có khả năng do quota/rate-limit (429) hay
 * lỗi tạm thời phía Google (5xx). Coi cả 2 bước (start + upload) như 1 đơn vị
 * thử lại — lỡ bước 1 thành công nhưng bước 2 lỗi retryable với key đó thì
 * vẫn chuyển hẳn sang key khác, làm lại từ đầu (không có API nào để "tiếp
 * tục" phiên upload dở dang bằng key KHÁC — URL upload gắn chặt với key đã
 * bắt đầu phiên đó).
 */
async function uploadFileWithFailover(keys: string[], mimeType: string, displayName: string, bytes: Uint8Array<ArrayBuffer>): Promise<Response> {
  const order = shuffle(keys);
  let lastResp: Response | null = null;

  for (const key of order) {
    const startResp = await fetch("https://generativelanguage.googleapis.com/upload/v1beta/files", {
      method: "POST",
      headers: {
        "x-goog-api-key": key,
        "X-Goog-Upload-Protocol": "resumable",
        "X-Goog-Upload-Command": "start",
        "X-Goog-Upload-Header-Content-Length": String(bytes.byteLength),
        "X-Goog-Upload-Header-Content-Type": mimeType,
        "content-type": "application/json",
      },
      body: JSON.stringify({ file: { display_name: displayName } }),
    });

    if (!startResp.ok) {
      lastResp = startResp;
      const retryable = startResp.status === 429 || startResp.status >= 500;
      if (!retryable) return startResp;
      continue;
    }

    const uploadUrl = startResp.headers.get("x-goog-upload-url");
    if (!uploadUrl) {
      lastResp = startResp;
      continue; // phản hồi bất thường (thiếu header cần thiết) -> thử key khác
    }

    const finishResp = await fetch(uploadUrl, {
      method: "POST",
      headers: {
        "X-Goog-Upload-Offset": "0",
        "X-Goog-Upload-Command": "upload, finalize",
        "content-length": String(bytes.byteLength),
      },
      body: bytes,
    });

    if (finishResp.ok) return finishResp;
    lastResp = finishResp;
    const retryable = finishResp.status === 429 || finishResp.status >= 500;
    if (!retryable) return finishResp;
  }

  return lastResp!;
}

/** Đọc 1 đoạn văn thành giọng nói bằng model TTS của Gemini (generateContent
 * với responseModalities AUDIO) — trả về PCM 16-bit dạng base64 kèm mime
 * (thường "audio/L16;codec=pcm;rate=24000"). Cùng kiểu failover key như
 * `callGeminiWithFailover`. Không stream: câu trả lời cần đọc thường ngắn,
 * app chỉ phát khi đã có đủ cả đoạn. */
async function synthesizeSpeechWithFailover(
  keys: string[],
  model: string,
  text: string,
  voice: string,
): Promise<Response> {
  const upstream = `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent`;
  const body = JSON.stringify({
    contents: [{ parts: [{ text }] }],
    generationConfig: {
      responseModalities: ["AUDIO"],
      speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: voice } } },
    },
  });

  let lastResp: Response | null = null;
  for (const key of shuffle(keys)) {
    const resp = await fetch(upstream, {
      method: "POST",
      headers: { "content-type": "application/json", "x-goog-api-key": key },
      body,
    });
    if (resp.ok) return resp;
    lastResp = resp;
    const retryable = resp.status === 429 || resp.status >= 500;
    if (!retryable) return resp;
  }
  return lastResp!;
}

/** Thời lượng tối đa 1 phiên trò chuyện trực tiếp — vừa kiểm soát quota
 * free-tier dùng chung, vừa kiểm soát thời gian Durable Object bị giữ sống
 * (tính phí theo GB-giây). */
const LIVE_MAX_SESSION_MS = 10 * 60 * 1000;

/** Mở WebSocket tới Gemini Live API, thử lần lượt các key — chuyển key khác
 * khi Google không nhận nâng cấp WebSocket (429/5xx hoặc lỗi bắt tay). */
async function connectUpstreamLive(keys: string[]): Promise<{ ws: WebSocket } | { status: number }> {
  let lastStatus = 0;
  for (const key of shuffle(keys)) {
    const resp = await fetch(
      `https://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=${encodeURIComponent(key)}`,
      { headers: { Upgrade: "websocket" } },
    );
    if (resp.webSocket) return { ws: resp.webSocket };
    lastStatus = resp.status;
    if (resp.status !== 429 && resp.status < 500) break;
  }
  return { status: lastStatus };
}

/** Mã đóng hợp lệ để GỬI đi (1005/1006/1015 chỉ được nhận, không được gửi). */
function sendableCloseCode(code: number): number {
  return code === 1000 || (code >= 3000 && code <= 4999) || (code >= 1001 && code <= 1014 && code !== 1005 && code !== 1006)
    ? code
    : 1011;
}

export class GeminiProxy implements DurableObject {
  // Không cần constructor lưu `state`/`env` — object này KHÔNG đọc/ghi
  // storage gì cả, chỉ tồn tại để ghim vị trí chạy (xem giải thích ở đầu
  // file). Tham số vẫn khai báo đủ theo interface DurableObject.
  constructor(_state: DurableObjectState, _env: Env) {}

  /** Nhận `model` qua header (tránh phải parse lại JSON body — body forward
   * NGUYÊN VĂN từ client, không đụng vào), `keys` qua header (JSON đã stringify
   * sẵn từ phía gọi), body chính là request body gốc gửi thẳng cho Gemini.
   *
   * `/upload` (URL nội bộ do index.ts tự đặt, xem đó) — nhánh RIÊNG cho
   * upload file lớn qua File API (xem `uploadFileWithFailover` ở trên), mọi
   * URL khác giữ nguyên hành vi cũ (streamGenerateContent). */
  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const keysHeader = request.headers.get("x-gemini-keys") ?? "[]";
    let keys: string[];
    try {
      keys = JSON.parse(keysHeader);
    } catch {
      keys = [];
    }

    if (url.pathname === "/upload") {
      const mimeType = request.headers.get("x-gemini-mime") ?? "";
      const filenameHeader = request.headers.get("x-gemini-filename") ?? "";
      let displayName = filenameHeader;
      try {
        displayName = decodeURIComponent(filenameHeader);
      } catch {
        // header không đúng percent-encoding -> dùng nguyên xi, không chặn cả upload chỉ vì tên hiển thị
      }
      if (!mimeType || keys.length === 0) {
        return new Response(JSON.stringify({ error: "GeminiProxy upload: thiếu mime hoặc keys" }), {
          status: 500,
          headers: { "content-type": "application/json" },
        });
      }
      const bytes = new Uint8Array(await request.arrayBuffer());
      const googleResp = await uploadFileWithFailover(keys, mimeType, displayName, bytes);
      if (!googleResp.ok) {
        const text = await googleResp.text();
        return new Response(text, { status: googleResp.status, headers: { "content-type": "application/json" } });
      }
      const data = await googleResp.json<{ file?: { uri?: string; mimeType?: string } }>();
      const fileUri = data.file?.uri;
      if (!fileUri) {
        return new Response(JSON.stringify({ error: "Google không trả về file.uri" }), {
          status: 502,
          headers: { "content-type": "application/json" },
        });
      }
      return new Response(JSON.stringify({ fileUri, mimeType: data.file?.mimeType ?? mimeType }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }

    // Trò chuyện trực tiếp (Gemini Live API, WebSocket 2 chiều) — mỗi phiên
    // 1 object riêng (index.ts đặt tên ngẫu nhiên) nhưng vẫn ghim "wnam",
    // cùng lý do chặn vùng như lệnh gọi thường. Object này chỉ chuyển tiếp
    // nguyên văn từng tin nhắn giữa app và Google, trừ tin nhắn `setup` đầu
    // tiên: ÉP model do server chọn + bỏ `tools` (client không được tự bật
    // công cụ tốn quota/ngoài phạm vi app).
    if (url.pathname === "/live") {
      if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
        return new Response("Cần nâng cấp WebSocket", { status: 426 });
      }
      const liveModel = request.headers.get("x-gemini-model") ?? "";
      if (!liveModel || keys.length === 0) {
        return new Response(JSON.stringify({ error: "GeminiProxy live: thiếu model hoặc keys" }), { status: 500 });
      }
      const upstreamResult = await connectUpstreamLive(keys);
      if (!("ws" in upstreamResult)) {
        return new Response(JSON.stringify({ error: `Không kết nối được Gemini Live (HTTP ${upstreamResult.status})` }), {
          status: 502,
          headers: { "content-type": "application/json" },
        });
      }
      const upstream = upstreamResult.ws;
      upstream.accept();

      const pair = new WebSocketPair();
      const [client, server] = Object.values(pair);
      server.accept();

      let setupSeen = false;
      let closed = false;
      const closeBoth = (code: number, reason: string) => {
        if (closed) return;
        closed = true;
        const c = sendableCloseCode(code);
        const r = reason.slice(0, 120);
        try {
          server.close(c, r);
        } catch {}
        try {
          upstream.close(c, r);
        } catch {}
      };

      server.addEventListener("message", (ev) => {
        let data = ev.data;
        if (!setupSeen) {
          // Tin nhắn ĐẦU TIÊN bắt buộc là `setup` — không phải thì đóng luôn.
          try {
            const msg = JSON.parse(typeof data === "string" ? data : new TextDecoder().decode(data as ArrayBuffer));
            if (!msg?.setup) throw new Error("no setup");
            msg.setup.model = `models/${liveModel}`;
            delete msg.setup.tools;
            data = JSON.stringify(msg);
            setupSeen = true;
          } catch {
            closeBoth(1008, "Tin nhắn đầu tiên phải là setup");
            return;
          }
        }
        try {
          upstream.send(data);
        } catch {
          closeBoth(1011, "Mất kết nối tới Gemini");
        }
      });
      upstream.addEventListener("message", (ev) => {
        try {
          server.send(ev.data);
        } catch {
          closeBoth(1011, "Mất kết nối tới app");
        }
      });
      server.addEventListener("close", () => closeBoth(1000, "App đã đóng phiên"));
      upstream.addEventListener("close", (ev) => closeBoth(ev.code, ev.reason || "Gemini đã đóng phiên"));
      server.addEventListener("error", () => closeBoth(1011, "Lỗi kết nối phía app"));
      upstream.addEventListener("error", () => closeBoth(1011, "Lỗi kết nối phía Gemini"));
      setTimeout(() => closeBoth(4000, "Hết thời lượng phiên (10 phút)"), LIVE_MAX_SESSION_MS);

      return new Response(null, { status: 101, webSocket: client });
    }

    if (url.pathname === "/tts") {
      const ttsModel = request.headers.get("x-gemini-model") ?? "";
      const payload = await request.json<{ text?: string; voice?: string }>().catch(() => ({}) as { text?: string; voice?: string });
      const text = (payload.text ?? "").trim();
      if (!ttsModel || keys.length === 0 || !text) {
        return new Response(JSON.stringify({ error: "GeminiProxy tts: thiếu model, keys hoặc text" }), {
          status: 400,
          headers: { "content-type": "application/json" },
        });
      }
      const googleResp = await synthesizeSpeechWithFailover(keys, ttsModel, text, payload.voice || "Kore");
      if (!googleResp.ok) {
        const errText = await googleResp.text();
        return new Response(errText, { status: googleResp.status, headers: { "content-type": "application/json" } });
      }
      const data = await googleResp.json<{
        candidates?: { content?: { parts?: { inlineData?: { mimeType?: string; data?: string } }[] } }[];
      }>();
      const part = data.candidates?.[0]?.content?.parts?.find((p) => p.inlineData?.data);
      if (!part?.inlineData?.data) {
        return new Response(JSON.stringify({ error: "Google không trả về dữ liệu âm thanh" }), {
          status: 502,
          headers: { "content-type": "application/json" },
        });
      }
      return new Response(
        JSON.stringify({ audio: part.inlineData.data, mimeType: part.inlineData.mimeType ?? "audio/L16;codec=pcm;rate=24000" }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }

    const model = request.headers.get("x-gemini-model") ?? "";
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
