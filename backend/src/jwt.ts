// JWT tự ký (HS256) để cấp "session token" riêng cho Snap AI, dùng Web Crypto
// API có sẵn trong Cloudflare Workers — KHÔNG cần thêm thư viện ngoài (giữ
// bundle nhỏ, ít bề mặt tấn công từ dependency).
//
// Lưu ý: đây là JWT do CHÍNH backend này ký và verify (không phải verify
// id_token của Google) — dùng để app chứng minh "tôi đã đăng nhập Google hợp
// lệ trước đó" với chính backend này ở các request sau, mà không cần đăng
// nhập lại Google mỗi lần gọi AI.

function base64url(input: string | Uint8Array): string {
  const bytes = typeof input === "string" ? new TextEncoder().encode(input) : input;
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function base64urlDecode(str: string): Uint8Array {
  const padded = str.replace(/-/g, "+").replace(/_/g, "/");
  const withPad = padded + "=".repeat((4 - (padded.length % 4)) % 4);
  const binary = atob(withPad);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

async function hmacKey(secret: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  );
}

export interface SessionPayload {
  sub: string; // Google user id ("sub" claim từ id_token Google)
  email: string;
  /** URL ảnh đại diện Google (claim "picture") — có thể thiếu (tài khoản
   * không đặt ảnh đại diện), UI phải tự có fallback (chữ cái đầu email). */
  picture?: string;
  iat: number;
  exp: number;
}

export async function signSession(payload: SessionPayload, secret: string): Promise<string> {
  const header = { alg: "HS256", typ: "JWT" };
  const signingInput = `${base64url(JSON.stringify(header))}.${base64url(JSON.stringify(payload))}`;
  const key = await hmacKey(secret);
  const sig = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(signingInput));
  return `${signingInput}.${base64url(new Uint8Array(sig))}`;
}

/** Verify chữ ký + hạn dùng. Trả về payload nếu hợp lệ, `null` nếu sai chữ ký
 * hoặc đã hết hạn (KHÔNG throw — gọi nơi khác chỉ cần check null/không-null). */
export async function verifySession(token: string, secret: string): Promise<SessionPayload | null> {
  const parts = token.split(".");
  if (parts.length !== 3) return null;
  const [encHeader, encPayload, encSig] = parts;

  const key = await hmacKey(secret);
  const valid = await crypto.subtle.verify(
    "HMAC",
    key,
    base64urlDecode(encSig).buffer as ArrayBuffer,
    new TextEncoder().encode(`${encHeader}.${encPayload}`),
  );
  if (!valid) return null;

  try {
    const payload = JSON.parse(new TextDecoder().decode(base64urlDecode(encPayload))) as SessionPayload;
    if (typeof payload.exp !== "number" || Date.now() / 1000 > payload.exp) return null;
    return payload;
  } catch {
    return null;
  }
}

/** Đọc phần payload của 1 JWT BẤT KỲ mà KHÔNG verify chữ ký — chỉ an toàn
 * dùng cho id_token nhận trực tiếp từ response HTTPS của chính Google (kênh
 * đã tin cậy sẵn), KHÔNG dùng cho token do client tự gửi lên (dễ giả mạo). */
export function decodeJwtPayloadUnsafe(token: string): Record<string, unknown> | null {
  const parts = token.split(".");
  if (parts.length < 2) return null;
  try {
    return JSON.parse(new TextDecoder().decode(base64urlDecode(parts[1])));
  } catch {
    return null;
  }
}
