# Snip-AI (Tauri)

Bản product thực sự của Snip-AI: Tauri (Rust backend, hiệu năng native, dùng
WebView2 có sẵn trên Windows — không đóng gói Chromium như Electron) + Svelte
5 + TailwindCSS cho giao diện.

Port đầy đủ tính năng từ bản MVP Python: hotkey `Ctrl+PrintScreen` → chụp +
đóng băng màn hình → kéo chọn vùng (dim nền, giữ sáng vùng chọn) → chip gợi ý
(OCR / Dịch / Tóm tắt / Giải thích) điền nhanh câu hỏi → hỏi AI streaming
(NVIDIA NIM hoặc Anthropic) → hỏi tiếp trong cùng cửa sổ kết quả.

## Cài đặt môi trường (làm 1 lần)

1. **Rust**: cài qua [rustup](https://www.rust-lang.org/tools/install) —
   chạy installer, chọn mặc định.
2. **Microsoft C++ Build Tools**: tải tại
   https://visualstudio.microsoft.com/visual-cpp-build-tools/ → khi cài, tick
   workload **"Desktop development with C++"**.
3. **WebView2 Runtime**: Windows 10/11 bản mới thường có sẵn (máy bạn đã có).
4. Kiểm tra: mở terminal MỚI (để nhận PATH vừa cài), chạy:
   ```powershell
   rustc --version
   cargo --version
   ```

## Chạy dev

```bash
cd snip-ai-tauri
npm install          # đã cài sẵn (kể cả Tailwind, plugin http/clipboard)
npm run tauri dev
```

Lần đầu `cargo` sẽ tải + compile toàn bộ dependency Rust — có thể mất vài
phút. Cửa sổ **"Snip-AI — Cài đặt"** sẽ hiện lên; nhập API key (NVIDIA hoặc
Anthropic) rồi bấm **Lưu cài đặt**. Bấm `Ctrl+PrintScreen` (hoặc nút "📸 Test
chụp màn hình" trong Settings nếu hotkey chưa đăng ký được) để thử luồng chụp.

## Kiến trúc & các quyết định kỹ thuật đáng chú ý

```
src-tauri/src/
  lib.rs        -> setup app, đăng ký plugin + global shortcut
  state.rs      -> AppState (Mutex) chia sẻ ảnh chụp/crop giữa các cửa sổ
  capture.rs     -> chụp màn hình (xcap) + crop ảnh (image crate)
  commands.rs    -> các #[tauri::command] gọi từ frontend + quản lý cửa sổ

src/
  routes/+page.svelte          -> cửa sổ chính: Cài đặt (provider, API key, model)
  routes/overlay/+page.svelte  -> cửa sổ overlay: dim màn hình + kéo chọn vùng
  routes/result/+page.svelte   -> cửa sổ kết quả: hỏi AI + chat streaming
  lib/aiClient.ts              -> gọi NVIDIA NIM / Anthropic streaming (SSE)
  lib/settings.ts              -> lưu cấu hình vào localStorage
  lib/config.ts                -> danh sách chip gợi ý (OCR/Dịch/Tóm tắt/Giải thích)
```

**1. Chụp "đóng băng" 1 lần, không dim real-time như bản Python.**
Khi bấm hotkey, Rust chụp toàn bộ primary monitor 1 lần (`xcap`), lưu vào
`AppState`, rồi mở cửa sổ overlay hiển thị chính tấm ảnh đó (làm nền, dim +
"khoét sáng" vùng đang chọn bằng CSS `clip-path`). Mượt hơn nhiều so với dim
overlay trong suốt thời gian thực, và tránh được vụ ảnh chụp bị dính chính cái
overlay của mình.

**2. Đơn vị PHYSICAL pixel xuyên suốt phần Rust.**
Ảnh chụp từ `xcap` là physical pixel (pixel thật của màn hình, không bị ảnh
hưởng bởi DPI scale của Windows). Mọi cửa sổ (overlay, result) được set vị
trí/kích thước bằng `PhysicalPosition`/`PhysicalSize` thay vì method
`.position()/.inner_size()` mặc định của builder (vốn nhận logical pixel) —
để tránh lệch vị trí/kích thước trên màn hình chạy DPI scale khác 100%
(rất phổ biến trên laptop Windows). **Điểm này mình chưa test được trên máy
DPI != 100% thật — nếu bạn thấy overlay/cửa sổ kết quả bị lệch, đây là chỗ
đầu tiên cần xem lại.**

**3. Gọi AI qua `@tauri-apps/plugin-http`, không phải `fetch` trình duyệt gốc.**
NVIDIA NIM (và nhiều API khác) không set CORS header cho phép gọi trực tiếp
từ 1 origin lạ như WebView của Tauri — gọi `fetch` gốc từ frontend sẽ bị chặn.
Plugin `http` của Tauri proxy request qua Rust (`reqwest`) ở backend nên không
bị giới hạn CORS, đồng thời vẫn hỗ trợ đọc response dạng stream để tự parse
SSE (`lib/aiClient.ts` tự parse `data: {...}` như OpenAI-style streaming).

**4. API key lưu ở `localStorage` (plaintext), chưa dùng OS keychain.**
Đơn giản, đủ dùng cho máy cá nhân. Muốn an toàn hơn cho bản phát hành rộng,
nên chuyển sang `tauri-plugin-stronghold` hoặc Windows Credential Manager.

**5. Cửa sổ overlay/result được "ẩn rồi tái dùng"** thay vì đóng hẳn mỗi lần
(`window.hide()` + event `overlay:reset`/`result:reset` để frontend tự reset
state) — tránh chi phí tạo lại WebView mỗi lần chụp, phản hồi nhanh hơn.

## Giới hạn đã biết (giống bản Python + thêm)

- Chỉ hỗ trợ **primary monitor** khi chụp — đa màn hình cần logic chọn monitor
  đang chứa con trỏ chuột (chưa làm).
- Chưa có system tray icon / chạy nền khi đóng cửa sổ Settings / tự khởi động
  cùng Windows — cửa sổ Settings hiện là cửa sổ "chính", đóng nó sẽ thoát app.
- Chưa test trên máy DPI scale != 100% (xem mục 2 ở trên).
- Chưa có xử lý khi ảnh crop quá lớn cho model (bản Python có nén/resize tự
  động cho NVIDIA — bản Tauri này CHƯA làm, cần bổ sung nếu gặp lỗi ảnh quá lớn).

## Việc tiếp theo đề xuất

1. Cài Rust + Build Tools, chạy `npm run tauri dev`, gửi mình log lỗi compile
   (nếu có) để sửa cùng nhau.
2. System tray icon (`tauri-plugin-tray` hoặc API tray built-in của Tauri v2)
   + ẩn thay vì đóng cửa sổ Settings.
3. Tự khởi động cùng Windows (`tauri-plugin-autostart`).
4. Hỗ trợ đa màn hình (chụp đúng monitor chứa con trỏ chuột).
5. Nén/resize ảnh crop trước khi gửi NVIDIA (như bản Python) để tránh lỗi ảnh
   quá lớn.
