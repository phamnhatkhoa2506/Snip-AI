// Đổi authorization code (app nhận được sau khi người dùng đăng nhập Google
// trong trình duyệt hệ thống) lấy id_token — bước này BẮT BUỘC làm ở backend,
// không phải ở app: cần `client_secret`, thứ không bao giờ được nhúng vào
// app phân phối cho người dùng (ai cũng lấy được từ file .exe).

export interface GoogleTokenResponse {
  id_token: string;
  access_token: string;
  expires_in: number;
}

export async function exchangeGoogleCode(
  clientId: string,
  clientSecret: string,
  code: string,
  codeVerifier: string,
  redirectUri: string,
): Promise<GoogleTokenResponse> {
  const params = new URLSearchParams({
    code,
    client_id: clientId,
    client_secret: clientSecret,
    redirect_uri: redirectUri,
    grant_type: "authorization_code",
    code_verifier: codeVerifier,
  });

  const resp = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: params.toString(),
  });

  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(`Google từ chối đổi authorization code (HTTP ${resp.status}): ${text}`);
  }

  return resp.json();
}
