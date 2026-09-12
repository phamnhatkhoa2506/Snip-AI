// Snap AI backend — Cloudflare Worker.
//
// Xác thực bằng "session token" (JWT tự ký, xem jwt.ts) cấp SAU KHI người
// dùng đăng nhập Google thành công (xem POST /v1/auth/google/exchange bên
// dưới) — app không còn cần API key riêng của người dùng nữa. `APP_SHARED_SECRET`
// vẫn được chấp nhận song song như 1 "cửa sau" tiện cho việc test bằng curl mà
// không cần chạy full luồng OAuth.

import { exchangeGoogleCode } from "./google";
import { decodeJwtPayloadUnsafe, signSession, verifySession, type SessionPayload } from "./jwt";

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
 * định dạng) là lỗi CHUNG cho mọi key — thử lại key khác vô ích, trả lỗi luôn.
 *
 * QUAN TRỌNG: chỉ đọc `resp.status` ở đây, CHƯA đụng vào `resp.body` — nhờ
 * vậy an toàn để "bỏ" response và thử key khác mà không làm hỏng stream (một
 * khi đã bắt đầu đọc/forward body thì không thể quay lại thử key khác nữa).
 */
async function callGeminiWithFailover(
  keys: string[],
  model: string,
  body: string,
): Promise<Response> {
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
      // forward nguyên xi, không parse/validate sâu ở bước 1. Model đọc từ
      // biến môi trường phía server (không cho client tự chọn model tuỳ ý,
      // tránh lạm dụng gọi model đắt tiền hơn).
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
      const model = wantsSearch
        ? env.GEMINI_SEARCH_MODEL || "gemini-2.5-flash"
        : env.GEMINI_MODEL || "gemini-3.6-flash";

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

      const resp = await callGeminiWithFailover(keys, model, body);

      // Forward thẳng response stream (kể cả lỗi) về client — giữ nguyên
      // status code + body, để logic xử lý lỗi/SSE phía Rust (friendly_error,
      // stream_sse) không cần đổi gì khi chuyển từ gọi thẳng Gemini sang gọi
      // qua backend này.
      return new Response(resp.body, {
        status: resp.status,
        headers: { "content-type": resp.headers.get("content-type") ?? "text/event-stream" },
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
