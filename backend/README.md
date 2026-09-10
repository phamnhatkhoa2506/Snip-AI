# Snap AI Backend

Proxy Cloudflare Worker cho Snap AI — giữ API key Gemini thật phía server,
app desktop không bao giờ cần biết/nhập API key nữa.

## Trạng thái hiện tại (Bước 1/2)

Chỉ có proxy gọi Gemini + xác thực tạm bằng 1 secret dùng chung
(`APP_SHARED_SECRET`). **Chưa có đăng nhập Google thật, chưa có giới hạn
quota theo người dùng** — đây là bản để test luồng proxy hoạt động đúng
trước khi làm bước 2 (đăng nhập Google + quota per-user), tránh dựng cả hệ
thống lớn rồi mới phát hiện lỗi luồng cơ bản.

## Setup lần đầu

```bash
cd backend
npm install

# Đăng nhập Cloudflare (mở trình duyệt xác thực)
npx wrangler login

# Đặt 2 secret — KHÔNG BAO GIỜ ghi vào wrangler.toml hay commit lên git
npx wrangler secret put GEMINI_API_KEYS
# dán 1 JSON ARRAY các API key (lấy từ https://aistudio.google.com/apikey,
# có thể tạo nhiều key từ nhiều tài khoản Google khác nhau để gộp quota
# free-tier lại) — VD: ["AIzaSy...key1","AIzaSy...key2","AIzaSy...key3"]
# Chỉ 1 key cũng được, vẫn phải bọc trong mảng: ["AIzaSy...key1"]
# Worker sẽ tự random chọn key mỗi request + tự chuyển key khác khi 1 key bị
# Google rate-limit (429) hoặc lỗi tạm thời (5xx).

npx wrangler secret put APP_SHARED_SECRET
# tự nghĩ 1 chuỗi ngẫu nhiên dài (VD `openssl rand -hex 32`), dùng tạm để
# app xác thực với backend — sẽ bị thay bằng đăng nhập Google thật ở bước 2
```

## Chạy thử local

Cloudflare Workers KHÔNG dùng `.env` truyền thống — có 2 nơi cấu hình tách
biệt hoàn toàn, không tự đồng bộ với nhau:

- **`.dev.vars`** (file, giống `.env`) — CHỈ dùng khi chạy `wrangler dev` local.
  Copy từ `.dev.vars.example`, điền giá trị thật, KHÔNG commit (đã có trong
  `.gitignore`).
- **`wrangler secret put`** — dùng cho production thật (`npm run deploy`), lưu
  mã hoá trên Cloudflare, không nằm trong file nào trên máy.

```bash
cp .dev.vars.example .dev.vars
# rồi sửa .dev.vars, điền GEMINI_API_KEYS + APP_SHARED_SECRET thật

npm run dev
```

Test bằng curl:

```bash
curl -N -X POST http://localhost:8787/v1/gemini/stream \
  -H "Authorization: Bearer <APP_SHARED_SECRET>" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{"role":"user","parts":[{"text":"Xin chào, bạn là ai?"}]}],
    "systemInstruction": {"parts":[{"text":"Trả lời ngắn gọn bằng tiếng Việt."}]}
  }'
```

## Deploy lên Cloudflare (production)

```bash
npm run deploy
```

Sau khi deploy, Wrangler in ra URL dạng
`https://snip-ai-backend.<subdomain>.workers.dev` — đó là endpoint app sẽ
gọi thay vì gọi thẳng Google.

## Kế hoạch bước 2 (chưa làm)

- Đăng nhập Google (OAuth 2.0, luồng "installed app" — Tauri mở trình duyệt hệ
  thống, nhận authorization code qua loopback `http://localhost:<port>`).
- Backend verify Google ID token, cấp session token riêng (JWT ký bằng secret
  của backend) cho app lưu (Windows Credential Manager, giống cách lưu API
  key trước đây).
- Đổi xác thực từ `APP_SHARED_SECRET` sang verify session token đó.
- Đếm quota theo user (Cloudflare KV hoặc D1): mỗi lượt gọi kiểm tra + tăng bộ
  đếm theo ngày, chặn nếu vượt hạn mức.
