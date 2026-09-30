// Snap AI backend — Cloudflare Worker.
//
// Xác thực bằng "session token" (JWT tự ký, xem jwt.ts) cấp SAU KHI người
// dùng đăng nhập Google thành công (xem POST /v1/auth/google/exchange bên
// dưới) — app không còn cần API key riêng của người dùng nữa. `APP_SHARED_SECRET`
// vẫn được chấp nhận song song như 1 "cửa sau" tiện cho việc test bằng curl mà
// không cần chạy full luồng OAuth.

import { exchangeGoogleCode } from "./google";
import { decodeJwtPayloadUnsafe, signSession, verifySession, type SessionPayload } from "./jwt";
import { GeminiProxy } from "./geminiProxy";

// Durable Object PHẢI được export từ module chính (Cloudflare tìm class theo
// đúng tên khai báo ở `wrangler.toml` trong module này) — `geminiProxy.ts`
// định nghĩa, ở đây chỉ export lại.
export { GeminiProxy };

export interface Env {
  // JSON array dạng chuỗi, VD '["key1","key2","key3"]' — nhiều key từ nhiều
  // tài khoản Google khác nhau để GỘP quota free-tier lại, phục vụ được nhiều
  // người dùng hơn mà không tốn tiền, đồng thời có key dự phòng khi 1 key bị
  // Google rate-limit (429). Đặt qua `wrangler secret put GEMINI_API_KEYS`.
  GEMINI_API_KEYS: string;
  GEMINI_MODEL: string;
  // Model dùng RIÊNG cho lượt hỏi có bật "Tra cứu web" (Google Search
  // grounding). Đã xác nhận qua trang giá chính thức của Google: grounding
  // KHÔNG khả dụng ở Free Tier cho dòng Gemini 3.x (GEMINI_MODEL hiện tại)
  // — nhưng Gemini 2.5 Flash lại CÓ free tier riêng cho grounding (500
  // lượt/ngày, miễn phí, không cần bật thanh toán). Optional với default bên
  // dưới nên không bắt buộc phải set lại secret/var cho deploy cũ.
  GEMINI_SEARCH_MODEL?: string;
  // Danh sách model app được tự chọn, phân tách dấu phẩy — optional, mặc
  // định xem DEFAULT_ALLOWED_CHAT_MODELS.
  GEMINI_ALLOWED_MODELS?: string;
  // Model đọc câu trả lời thành giọng nói (POST /v1/gemini/tts) — xem
  // wrangler.toml. Optional, có default trong code.
  GEMINI_TTS_MODEL?: string;
  // Model trò chuyện trực tiếp bằng giọng nói (GET /v1/gemini/live, WebSocket).
  GEMINI_LIVE_MODEL?: string;

  // OAuth Client ID/Secret lấy từ Google Cloud Console (loại "Desktop app").
  // Client ID KHÔNG bí mật (nhúng thẳng vào app), Client Secret PHẢI giữ bí
  // mật (chỉ backend này biết, dùng để đổi authorization code -> id_token).
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;

  // Secret riêng của backend, dùng để KÝ session token cấp cho app sau khi
  // đăng nhập Google thành công — khác hoàn toàn với client secret của Google.
  APP_SESSION_SECRET: string;

  // "Cửa sau" tạm cho việc test bằng curl (xem giải thích ở đầu file). Có thể
  // bỏ hẳn field này khi luồng OAuth đã chạy ổn định trong production.
  APP_SHARED_SECRET?: string;

  // Lưu câu trả lời khảo sát mức độ hài lòng (POST /v1/survey) — xem
  // wrangler.toml. KV đơn giản là đủ: mỗi lượt gửi 1 key riêng, không cần
  // đọc lại/query có cấu trúc (đọc lại bằng `wrangler kv key list`/dashboard
  // lúc cần xem, không qua API này).
  SURVEY_KV: KVNamespace;

  // Durable Object ghim CỨNG vị trí chạy (locationHint) khi gọi Gemini — xem
  // giải thích đầy đủ ở đầu geminiProxy.ts. Thay cho việc gọi thẳng Gemini từ
  // Worker chính (Smart Placement ở wrangler.toml không đảm bảo, đã gặp thực
  // tế bị Google chặn vùng dù người dùng ở vùng được hỗ trợ).
  // Không cần tham số generic `<GeminiProxy>` — chỉ gọi `.fetch()` thường
  // (không dùng RPC method trực tiếp trên class), không cần "brand" class.
  GEMINI_PROXY: DurableObjectNamespace;
}

/** Model chat app được phép TỰ CHỌN (header `x-snap-model`, xem Cài đặt ->
 * "Mô hình AI" trong app). Chỉ gồm model ỔN ĐỊNH còn FREE TIER — key ở đây
 * là key free-tier dùng chung, cho chọn model trả phí (dòng Pro) hay model
 * đã bị Google khoá (dòng 2.5 cho người dùng mới) thì bấm vào là lỗi. Model
 * ngoài danh sách -> lặng lẽ dùng GEMINI_MODEL mặc định. Ghi đè được bằng
 * biến GEMINI_ALLOWED_MODELS (danh sách phân tách bằng dấu phẩy) khi Google
 * ra model mới mà chưa kịp cập nhật code. Phải khớp danh sách
 * GEMINI_MODEL_OPTIONS trong app (src/lib/settings.ts). */
const DEFAULT_ALLOWED_CHAT_MODELS = [
  "gemini-3.8-flash",
  "gemini-3.7-flash",
  "gemini-3.6-flash",
  "gemini-3.5-flash",
  "gemini-3.5-flash-lite",
  "gemini-3.1-flash-lite",
];

/** Model đọc giọng/trò chuyện trực tiếp app được tự chọn — khớp
 * TTS_MODEL_OPTIONS/LIVE_MODEL_OPTIONS trong app (src/lib/settings.ts). */
const ALLOWED_TTS_MODELS = ["gemini-3.8-flash-lite-tts", "gemini-3.8-flash-tts"];
const ALLOWED_LIVE_MODELS = ["gemini-3.8-live", "gemini-3.8-live-extended-thinking"];

function pickFromList(wanted: string | null | undefined, allowed: string[], fallback: string): string {
  const w = (wanted ?? "").trim();
  return allowed.includes(w) ? w : fallback;
}

function pickChatModel(request: Request, env: Env): string {
  const fallback = env.GEMINI_MODEL || "gemini-3.6-flash";
  const wanted = (request.headers.get("x-snap-model") ?? "").trim();
  if (!wanted) return fallback;
  const allowed = env.GEMINI_ALLOWED_MODELS
    ? env.GEMINI_ALLOWED_MODELS.split(",").map((m) => m.trim()).filter(Boolean)
    : DEFAULT_ALLOWED_CHAT_MODELS;
  return allowed.includes(wanted) ? wanted : fallback;
}

function unauthorized(message = "Unauthorized"): Response {
  return new Response(JSON.stringify({ error: message }), {
    status: 401,
    headers: { "content-type": "application/json" },
  });
}

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), { status, headers: { "content-type": "application/json" } });
}

/** Xác thực request tới các endpoint cần đăng nhập. Chấp nhận 1 trong 2:
 * - Session token (JWT ký bởi chính backend này, cấp qua /v1/auth/google/exchange).
 * - `APP_SHARED_SECRET` dùng chung (cửa sau tiện test, xem giải thích ở đầu file).
 * Trả về thông tin người dùng nếu hợp lệ, `null` nếu không. */
async function authenticate(request: Request, env: Env): Promise<{ userId: string; email: string } | null> {
  const auth = request.headers.get("Authorization") ?? "";
  const token = auth.startsWith("Bearer ") ? auth.slice("Bearer ".length) : "";
  if (!token) return null;

  if (env.APP_SHARED_SECRET && token === env.APP_SHARED_SECRET) {
    return { userId: "dev-shared-secret", email: "dev@local" };
  }

  const session = await verifySession(token, env.APP_SESSION_SECRET);
  if (session) return { userId: session.sub, email: session.email };

  return null;
}

// `shuffle`/`callGeminiWithFailover` đã CHUYỂN sang geminiProxy.ts — lệnh gọi
// Gemini thật giờ chạy TRONG Durable Object (ghim cứng vị trí), không còn gọi
// thẳng từ đây nữa. Xem `GEMINI_PROXY` trong Env + giải thích ở geminiProxy.ts.

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/health") {
      return new Response("ok");
    }

    // Đổi authorization code (app nhận được sau khi người dùng đăng nhập
    // Google trong trình duyệt hệ thống) lấy session token của backend này.
    // Xem luồng đầy đủ trong README.md ("Luồng đăng nhập Google").
    if (url.pathname === "/v1/auth/google/exchange" && request.method === "POST") {
      let payload: { code?: string; codeVerifier?: string; redirectUri?: string };
      try {
        payload = await request.json();
      } catch {
        return json({ error: "Body phải là JSON" }, 400);
      }
      const { code, codeVerifier, redirectUri } = payload;
      if (!code || !codeVerifier || !redirectUri) {
        return json({ error: "Thiếu code/codeVerifier/redirectUri" }, 400);
      }

      let tokenResp;
      try {
        tokenResp = await exchangeGoogleCode(env.GOOGLE_CLIENT_ID, env.GOOGLE_CLIENT_SECRET, code, codeVerifier, redirectUri);
      } catch (e) {
        return json({ error: `Đăng nhập Google thất bại: ${(e as Error).message}` }, 401);
      }

      // id_token nhận TRỰC TIẾP từ response HTTPS của chính Google ở bước
      // trên (kênh đã tin cậy) -> đọc payload không cần verify lại chữ ký.
      // Vẫn kiểm tra `aud` khớp đúng Client ID của app này, phòng trường hợp
      // cấu hình nhầm/dùng lẫn token của app khác.
      const idPayload = decodeJwtPayloadUnsafe(tokenResp.id_token);
      if (!idPayload || idPayload.aud !== env.GOOGLE_CLIENT_ID) {
        return json({ error: "id_token không hợp lệ (sai audience)" }, 401);
      }
      const sub = idPayload.sub as string | undefined;
      const email = idPayload.email as string | undefined;
      const picture = idPayload.picture as string | undefined;
      if (!sub || !email) {
        return json({ error: "id_token thiếu thông tin người dùng" }, 401);
      }

      const now = Math.floor(Date.now() / 1000);
      const THIRTY_DAYS = 30 * 24 * 60 * 60;
      const session: SessionPayload = { sub, email, picture, iat: now, exp: now + THIRTY_DAYS };
      const sessionToken = await signSession(session, env.APP_SESSION_SECRET);

      return json({ sessionToken, email, picture });
    }

    if (url.pathname === "/v1/gemini/stream" && request.method === "POST") {
      const user = await authenticate(request, env);
      if (!user) return unauthorized();

      // Body request client gửi lên PHẢI đúng format Gemini generateContent
      // (contents, systemInstruction, generationConfig...) — worker này chỉ
      // forward nguyên xi, không parse/validate sâu ở bước 1. Model: người
      // dùng chọn trong app (header x-snap-model) nhưng CHỈ trong danh sách
      // cho phép (xem pickChatModel) — không cho client gọi model tuỳ ý.
      const body = await request.text();

      // Lượt hỏi có bật "Tra cứu web" (field "tools" chứa google_search) ->
      // BẮT BUỘC đổi sang model có free tier cho grounding (xem giải thích ở
      // GEMINI_SEARCH_MODEL) — dùng GEMINI_MODEL mặc định (Gemini 3.x) sẽ bị
      // Google chặn thẳng vì grounding không khả dụng ở free tier cho dòng
      // đó, bất kể còn dư quota bao nhiêu. Chỉ cần dò chuỗi con trong body
      // thô (không parse JSON đầy đủ) — Rust luôn ghi đúng 1 trong 2 tên field
      // này khi bật search (xem ai.rs), đủ tin cậy cho việc CHỌN MODEL, không
      // ảnh hưởng gì tới nội dung request thật sự forward đi.
      const wantsSearch = body.includes('"googleSearch"') || body.includes('"google_search"');
      const model = wantsSearch ? env.GEMINI_SEARCH_MODEL || "gemini-2.5-flash" : pickChatModel(request, env);

      let keys: string[];
      try {
        keys = JSON.parse(env.GEMINI_API_KEYS);
        if (!Array.isArray(keys) || keys.length === 0) throw new Error("empty");
      } catch {
        return new Response(
          JSON.stringify({ error: "Backend chưa cấu hình đúng GEMINI_API_KEYS (phải là JSON array khác rỗng)" }),
          { status: 500, headers: { "content-type": "application/json" } },
        );
      }

      // Forward toàn bộ việc gọi Gemini sang Durable Object đã GHIM CỨNG vị
      // trí chạy ("wnam", gần cụm server Gemini nhất) — xem giải thích đầy đủ
      // ở geminiProxy.ts. Dùng CHUNG 1 tên cố định ("gemini") cho mọi request
      // -> luôn cùng 1 instance, cùng 1 vị trí đã ghim từ lần đầu tạo, không
      // phụ thuộc traffic/thời điểm như Smart Placement của Worker chính.
      const proxy = env.GEMINI_PROXY.getByName("gemini", { locationHint: "wnam" });
      const resp = await proxy.fetch("https://gemini-proxy.internal/stream", {
        method: "POST",
        headers: { "x-gemini-model": model, "x-gemini-keys": JSON.stringify(keys) },
        body,
      });

      // Forward thẳng response stream (kể cả lỗi) về client — giữ nguyên
      // status code + body, để logic xử lý lỗi/SSE phía Rust (friendly_error,
      // stream_sse) không cần đổi gì khi chuyển từ gọi thẳng Gemini sang gọi
      // qua backend này.
      return new Response(resp.body, {
        status: resp.status,
        headers: { "content-type": resp.headers.get("content-type") ?? "text/event-stream" },
      });
    }

    // Upload 1 file đính kèm LỚN (ảnh/PDF, xem attachments.rs + file_api.rs
    // phía Rust) qua Gemini File API — chỉ nhận RAW BYTES làm body (không
    // phải JSON) để khỏi phải base64-hoá thêm 1 lớp nữa cho vô ích (client đã
    // gửi thẳng bytes gốc). `x-gemini-mime`/`x-gemini-filename` mang theo
    // metadata cần thiết vì body không còn chỗ chứa gì khác ngoài bytes.
    if (url.pathname === "/v1/gemini/upload" && request.method === "POST") {
      const user = await authenticate(request, env);
      if (!user) return unauthorized();

      const mimeType = request.headers.get("x-gemini-mime") ?? "";
      const filenameHeader = request.headers.get("x-gemini-filename") ?? "file";
      if (!mimeType) {
        return json({ error: "Thiếu header x-gemini-mime" }, 400);
      }

      const bytes = new Uint8Array(await request.arrayBuffer());
      if (bytes.byteLength === 0) {
        return json({ error: "File rỗng" }, 400);
      }

      let keys: string[];
      try {
        keys = JSON.parse(env.GEMINI_API_KEYS);
        if (!Array.isArray(keys) || keys.length === 0) throw new Error("empty");
      } catch {
        return json({ error: "Backend chưa cấu hình đúng GEMINI_API_KEYS (phải là JSON array khác rỗng)" }, 500);
      }

      // Cùng Durable Object GHIM CỨNG vị trí ("wnam") với lệnh hỏi AI chính
      // (xem geminiProxy.ts) — upload cũng gọi thẳng Google, dính CÙNG lỗi
      // chặn vùng nếu không ghim vị trí y hệt.
      const proxy = env.GEMINI_PROXY.getByName("gemini", { locationHint: "wnam" });
      const resp = await proxy.fetch("https://gemini-proxy.internal/upload", {
        method: "POST",
        headers: {
          "x-gemini-keys": JSON.stringify(keys),
          "x-gemini-mime": mimeType,
          "x-gemini-filename": filenameHeader,
        },
        body: bytes,
      });

      return new Response(resp.body, {
        status: resp.status,
        headers: { "content-type": resp.headers.get("content-type") ?? "application/json" },
      });
    }

    // Trò chuyện trực tiếp bằng giọng nói — nâng cấp WebSocket rồi giao hẳn
    // cho 1 Durable Object RIÊNG của phiên này (tên ngẫu nhiên), vẫn ghim
    // "wnam". Không dùng chung object "gemini" như các lệnh khác: 1 phiên giữ
    // kết nối tới 10 phút, dồn mọi phiên vào 1 object (chạy đơn luồng) sẽ làm
    // chậm tất cả.
    if (url.pathname === "/v1/gemini/live") {
      if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
        return json({ error: "Endpoint này chỉ nhận WebSocket" }, 426);
      }
      const user = await authenticate(request, env);
      if (!user) return unauthorized();

      let keys: string[];
      try {
        keys = JSON.parse(env.GEMINI_API_KEYS);
        if (!Array.isArray(keys) || keys.length === 0) throw new Error("empty");
      } catch {
        return json({ error: "Backend chưa cấu hình đúng GEMINI_API_KEYS (phải là JSON array khác rỗng)" }, 500);
      }

      const proxy = env.GEMINI_PROXY.getByName(`live-${crypto.randomUUID()}`, { locationHint: "wnam" });
      return proxy.fetch("https://gemini-proxy.internal/live", {
        headers: {
          Upgrade: "websocket",
          "x-gemini-keys": JSON.stringify(keys),
          "x-gemini-model": pickFromList(
            request.headers.get("x-snap-model"),
            ALLOWED_LIVE_MODELS,
            env.GEMINI_LIVE_MODEL || "gemini-3.8-live",
          ),
        },
      });
    }

    // Đọc câu trả lời thành giọng nói (Snap Audio / hỏi bằng giọng) — cùng
    // Durable Object ghim vị trí như các lệnh gọi Gemini khác.
    if (url.pathname === "/v1/gemini/tts" && request.method === "POST") {
      const user = await authenticate(request, env);
      if (!user) return unauthorized();

      let payload: { text?: string; voice?: string; model?: string };
      try {
        payload = await request.json();
      } catch {
        return json({ error: "Body phải là JSON" }, 400);
      }
      // Chặn đoạn quá dài — vừa tốn quota TTS vừa phát lâu vô ích; app đã tự
      // rút gọn trước khi gửi (xem speech.ts), đây chỉ là lớp chặn cuối.
      const text = (payload.text ?? "").trim().slice(0, 4000);
      if (!text) return json({ error: "Thiếu text" }, 400);
      const voice = /^[A-Za-z]{2,32}$/.test(payload.voice ?? "") ? payload.voice : undefined;

      let keys: string[];
      try {
        keys = JSON.parse(env.GEMINI_API_KEYS);
        if (!Array.isArray(keys) || keys.length === 0) throw new Error("empty");
      } catch {
        return json({ error: "Backend chưa cấu hình đúng GEMINI_API_KEYS (phải là JSON array khác rỗng)" }, 500);
      }

      const proxy = env.GEMINI_PROXY.getByName("gemini", { locationHint: "wnam" });
      const resp = await proxy.fetch("https://gemini-proxy.internal/tts", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-gemini-keys": JSON.stringify(keys),
          "x-gemini-model": pickFromList(
            payload.model,
            ALLOWED_TTS_MODELS,
            env.GEMINI_TTS_MODEL || "gemini-3.8-flash-lite-tts",
          ),
        },
        body: JSON.stringify({ text, voice }),
      });
      return new Response(resp.body, {
        status: resp.status,
        headers: { "content-type": resp.headers.get("content-type") ?? "application/json" },
      });
    }

    // Khảo sát mức độ hài lòng — KHÔNG bắt buộc đăng nhập (người dùng chưa
    // đăng nhập Google, đang tự dùng API key riêng, vẫn nên khảo sát được
    // bình thường). Có đăng nhập thì gắn kèm email để biết ai gửi, không thì
    // lưu ẩn danh — cả 2 đều hợp lệ, không trả lỗi 401 ở endpoint này.
    if (url.pathname === "/v1/survey" && request.method === "POST") {
      let payload: { rating?: string; comment?: string; appVersion?: string };
      try {
        payload = await request.json();
      } catch {
        return json({ error: "Body phải là JSON" }, 400);
      }

      const VALID_RATINGS = ["unhappy", "happy", "very_happy"];
      const rating = payload.rating ?? "";
      if (!VALID_RATINGS.includes(rating)) {
        return json({ error: `rating phải là 1 trong: ${VALID_RATINGS.join(", ")}` }, 400);
      }
      // Chặn payload quá khổ (spam/lỗi client) — góp ý thật sự không cần dài
      // hơn vài đoạn văn.
      const comment = (payload.comment ?? "").slice(0, 2000);
      const appVersion = (payload.appVersion ?? "").slice(0, 40);

      const user = await authenticate(request, env);
      const record = {
        rating,
        comment,
        appVersion,
        email: user?.email ?? null,
        submittedAt: new Date().toISOString(),
      };

      // Key ngẫu nhiên (không cần đọc lại theo thứ tự/tra cứu gì từ app) —
      // đủ để không đè lên nhau khi nhiều người gửi cùng lúc.
      const key = `survey:${Date.now()}:${crypto.randomUUID()}`;
      await env.SURVEY_KV.put(key, JSON.stringify(record));

      return json({ ok: true });
    }

    return new Response("Not found", { status: 404 });
  },
};
