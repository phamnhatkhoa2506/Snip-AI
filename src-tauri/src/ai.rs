//! Gọi AI (NVIDIA NIM / OpenAI / Anthropic / Gemini) TRỰC TIẾP từ Rust bằng
//! `reqwest`, thay vì qua `@tauri-apps/plugin-http` (fetch ở JS). Lý do:
//!
//! - Ảnh đã crop đã nằm sẵn trong `AppState` (Rust) — không cần base64-hoá rồi
//!   gửi ngược qua IPC cho JS chỉ để JS lại gửi nó qua IPC lần nữa cho lệnh
//!   `fetch` của plugin-http xử lý. Làm thẳng trong Rust bỏ hẳn 1 lượt IPC
//!   payload lớn (từng là nguyên nhân treo request trong `tauri dev`, nơi IPC
//!   phải fallback qua kênh `postMessage` có giới hạn kích thước).
//! - API KEY không bao giờ đi qua frontend: đọc thẳng từ OS keychain
//!   (secrets.rs). Xem thêm giải thích bảo mật ở đầu file secrets.rs.
//! - Mỗi đoạn text nhận được từ AI được emit thẳng ra window bằng sự kiện nhỏ
//!   (`ai:delta`), IPC payload mỗi lần chỉ vài chục byte, không bao giờ lớn.
//!
//! NVIDIA NIM và OpenAI dùng chung 1 định dạng API (OpenAI Chat Completions
//! với `image_url` content-part) -> gộp chung logic ở `ask_openai_compatible`.
//! Anthropic và Gemini có định dạng riêng, tách hàm riêng.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::secrets;
use crate::state::{AppState, HttpClientState};

/// Thời gian tối đa CHỜ PHẢN HỒI ĐẦU TIÊN (header) — không giới hạn tổng thời
/// gian đọc hết stream, vì model sinh chậm vẫn nên cho chạy tiếp. Model lớn
/// trên API serverless free-tier có thể "cold start" mất 1-2 phút cho lần gọi
/// đầu — 150s đủ rộng để không báo lỗi oan, nhưng vẫn có giới hạn thay vì treo
/// vô thời hạn im lặng.
const FIRST_RESPONSE_TIMEOUT: Duration = Duration::from_secs(150);

/// System prompt dùng chung cho mọi provider — ép model trả lời gọn, có cấu
/// trúc Markdown, và KHÔNG có câu dẫn thừa ("Chắc chắn rồi, dưới đây là...").
/// Trước đây không có system prompt nên output mỗi lần một kiểu, hay kèm lời
/// dẫn và mô tả lại yêu cầu.
///
/// ⚠️ Đã cố tình bỏ cụm "vùng ảnh vừa cắt từ màn hình" trong bản trước — phát
/// hiện thực tế: model nhỏ (VD llama-3.2-11b-vision) khi không đọc được nội
/// dung ảnh có xu hướng "mượn" luôn từ ngữ trong system prompt để bịa ra câu
/// trả lời nghe hợp lý (VD tự bịa đoạn text về "vùng cắt ảnh" — đúng cụm từ
/// lấy từ prompt — dù ảnh không hề nói về chủ đề đó). Rút gọn tối đa + thêm
/// quy tắc chống bịa rõ ràng để giảm rủi ro này.
const SYSTEM_PROMPT: &str = "\
Bạn là trợ lý AI phân tích ảnh, tích hợp trong app Snap AI.

QUY TẮC (bắt buộc):
1. CHỈ mô tả/trả lời dựa trên những gì THỰC SỰ thấy trong ảnh. Nếu ảnh mờ, quá nhỏ, \
hoặc không đọc rõ được nội dung, PHẢI nói thẳng điều đó (VD \"Ảnh quá mờ để đọc chữ\") \
— TUYỆT ĐỐI không bịa/đoán nội dung để có câu trả lời nghe hợp lý.
2. Vào thẳng nội dung, không mở đầu bằng \"Chắc chắn rồi\", \"Dưới đây là\", \"Trong ảnh \
này tôi thấy\" hay bất kỳ lời dẫn nào. Không nhắc lại yêu cầu của người dùng. Không thêm \
lời kết thừa kiểu \"Hy vọng giúp ích\".
3. Trả lời bằng tiếng Việt, TRỪ KHI người dùng yêu cầu ngôn ngữ khác, hoặc yêu cầu trích \
xuất/giữ nguyên văn bản gốc (khi đó giữ đúng ngôn ngữ gốc).
4. Dùng Markdown để cấu trúc câu trả lời: **in đậm** cho ý chính, danh sách gạch đầu dòng \
cho nhiều ý, `code` cho mã/lệnh/tên file/giá trị kỹ thuật, khối ```code``` cho đoạn mã \
nhiều dòng, bảng Markdown khi dữ liệu có dạng bảng.
5. Ngắn gọn, đúng trọng tâm — độ dài tương xứng với nội dung, không dài dòng.";

#[derive(Deserialize, Clone)]
pub struct ChatTurnDto {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

#[derive(Serialize, Clone)]
struct DeltaPayload {
    piece: String,
}

/// Đọc ảnh của ĐÚNG phiên đang gọi (nhiều cửa sổ "Kết quả AI" có thể mở cùng
/// lúc, mỗi cửa sổ 1 ảnh riêng — xem giải thích ở `AppState::crop_sessions`).
fn get_crop_base64(state: &State<'_, AppState>, window_label: &str) -> Result<String, String> {
    let sessions = state.crop_sessions.lock().unwrap();
    let bytes = sessions
        .get(window_label)
        .ok_or("Không tìm thấy ảnh cho phiên này (cửa sổ có thể đã bị đóng)")?;
    Ok(STANDARD.encode(bytes))
}

async fn send_with_timeout(req: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    match tokio::time::timeout(FIRST_RESPONSE_TIMEOUT, req.send()).await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Err(format!("Lỗi gửi request: {e}")),
        Err(_) => Err(format!(
            "Hết thời gian chờ phản hồi sau {}s (có thể model đang cold-start hoặc mạng có vấn đề).",
            FIRST_RESPONSE_TIMEOUT.as_secs()
        )),
    }
}

/// Đọc từng chunk bytes từ response stream, tách theo dòng SSE ("data: ..."),
/// gọi `on_piece` cho mỗi mẩu text nhận được (rút ra bằng `extract`). Trả về
/// toàn bộ text ghép lại.
async fn stream_sse<S, F>(
    mut stream: S,
    mut on_piece: F,
    extract: impl Fn(&serde_json::Value) -> Option<String>,
) -> Result<String, String>
where
    S: futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
    F: FnMut(&str),
{
    let mut buffer = String::new();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Lỗi đọc stream: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue; // dòng SSE không parse được -> bỏ qua, không phải lỗi nghiêm trọng
            };
            if let Some(piece) = extract(&json) {
                if !piece.is_empty() {
                    full.push_str(&piece);
                    on_piece(&piece);
                }
            }
        }
    }

    Ok(full)
}

/// Rút gọn body lỗi HTTP (thường là JSON) thành 1 câu dễ đọc, thay vì đẩy
/// nguyên JSON thô lên UI (đã gặp thực tế: NVIDIA trả JSON kiểu RFC 7807
/// problem+json, OpenAI/Gemini dùng {"error":{"message":...}}, mỗi provider
/// một kiểu). Thử lần lượt các field hay gặp; nếu không parse được thì rơi về
/// hiển thị đoạn text gốc (cắt bớt nếu quá dài) thay vì lỗi trắng tay.
fn friendly_error(provider_label: &str, status: reqwest::StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("detail")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| json["error"]["message"].as_str().map(String::from))
                .or_else(|| json.get("message").and_then(|v| v.as_str()).map(String::from))
                .or_else(|| json.get("title").and_then(|v| v.as_str()).map(String::from))
        })
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                "(không có nội dung lỗi chi tiết)".to_string()
            } else if trimmed.chars().count() > 220 {
                format!("{}…", trimmed.chars().take(220).collect::<String>())
            } else {
                trimmed.to_string()
            }
        });
    format!("{provider_label} báo lỗi (HTTP {status}): {message}")
}

fn finalize(full: String) -> String {
    let trimmed = full.trim();
    if trimmed.is_empty() {
        "(AI không trả về nội dung)".to_string()
    } else {
        trimmed.to_string()
    }
}

/// NVIDIA NIM và OpenAI đều dùng định dạng "OpenAI Chat Completions" —
/// messages với content dạng mảng [{type:text}, {type:image_url}], stream SSE
/// trả `choices[0].delta.content`. Gộp chung logic gọi API ở đây.
async fn ask_openai_compatible(
    app: &AppHandle,
    state: &State<'_, AppState>,
    window_label: &str,
    endpoint: &str,
    provider: &str,
    model: &str,
    extra_body: serde_json::Value,
    history: &[ChatTurnDto],
) -> Result<String, String> {
    let api_key = secrets::read_api_key(provider)?;
    let model = model.trim();
    let img_b64 = get_crop_base64(state, window_label)?;

    // Chỉ đính ảnh vào LƯỢT USER GẦN NHẤT, không phải mọi lượt user trong lịch
    // sử. Trước đây gửi lại ảnh ở TẤT CẢ các lượt user (để tránh model mất
    // "grounding" ở câu hỏi tiếp theo) — nhưng cách đó làm payload phình to
    // dần theo cấp số cộng: hỏi lần 4 sẽ gửi lại y hệt tấm ảnh đó 4 lần trong
    // 1 request, cực kỳ tốn băng thông + thời gian upload, đây là 1 nguyên
    // nhân chính khiến app "phản hồi chậm dần" khi chat dài. Chỉ lượt mới nhất
    // cần ảnh vẫn đủ để model giữ grounding, vì đó luôn là câu hỏi đang chờ trả lời.
    let last_user_idx = history.iter().rposition(|t| t.role == "user");
    let mut messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": SYSTEM_PROMPT})];
    messages.extend(history.iter().enumerate().map(|(i, turn)| {
        if turn.role == "user" && Some(i) == last_user_idx {
            serde_json::json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": turn.content},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{img_b64}")}},
                ],
            })
        } else {
            serde_json::json!({"role": turn.role, "content": turn.content})
        }
    }));

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 1500,
        "stream": true,
        // Chặn lặp — đã gặp thực tế: model nhỏ (VD llama-3.2-11b-vision) đôi
        // khi rơi vào vòng lặp sinh y hệt từng đoạn/câu (đặc biệt với ảnh
        // dày đặc chữ mà model không đọc chắc chắn được, nó có xu hướng bịa
        // nội dung rồi lặp lại chính mình). frequency_penalty là tham số
        // chuẩn OpenAI-compatible, được hầu hết backend vLLM (bao gồm NIM)
        // hỗ trợ, phạt các token đã xuất hiện nhiều lần trong response.
        "frequency_penalty": 0.4,
        "messages": messages,
    });
    if let (Some(body_obj), Some(extra_obj)) = (body.as_object_mut(), extra_body.as_object()) {
        for (k, v) in extra_obj {
            body_obj.insert(k.clone(), v.clone());
        }
    }

    eprintln!("[snip-ai][ai] POST {endpoint} (model={model})");
    let client = &app.state::<HttpClientState>().client;
    let req = client
        .post(endpoint)
        .bearer_auth(&api_key)
        .header("Accept", "text/event-stream")
        .json(&body);
    let resp = send_with_timeout(req).await?;

    eprintln!("[snip-ai][ai] response: status={}", resp.status());
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        eprintln!("[snip-ai][ai] LỖI body: {text}");
        let label = if provider == "nvidia" { "NVIDIA NIM" } else { "OpenAI" };
        return Err(friendly_error(label, status, &text));
    }

    let window_label = window_label.to_string();
    let app = app.clone();
    let full = stream_sse(
        resp.bytes_stream(),
        move |piece| {
            let _ = app.emit_to(&window_label, "ai:delta", DeltaPayload { piece: piece.to_string() });
        },
        |json| json["choices"][0]["delta"]["content"].as_str().map(|s| s.to_string()),
    )
    .await?;

    Ok(finalize(full))
}

#[tauri::command]
pub async fn ask_ai_nvidia(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    model: String,
    // `None` (hoặc chuỗi rỗng) = KHÔNG gửi tham số này lên API. Nhiều model
    // trên NVIDIA NIM (VD llama-3.2-11b-vision-instruct) không hỗ trợ khái
    // niệm "reasoning" — chỉ gửi param này khi người dùng chủ động bật, tránh
    // gửi 1 tham số vô nghĩa/không được hỗ trợ cho model không cần.
    reasoning_effort: Option<String>,
    history: Vec<ChatTurnDto>,
) -> Result<String, String> {
    let extra = match reasoning_effort.filter(|s| !s.trim().is_empty()) {
        Some(effort) => serde_json::json!({ "reasoning_effort": effort }),
        None => serde_json::json!({}),
    };
    ask_openai_compatible(
        &app,
        &state,
        &window_label,
        "https://integrate.api.nvidia.com/v1/chat/completions",
        "nvidia",
        &model,
        extra,
        &history,
    )
    .await
}

#[tauri::command]
pub async fn ask_ai_openai(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    model: String,
    history: Vec<ChatTurnDto>,
) -> Result<String, String> {
    ask_openai_compatible(
        &app,
        &state,
        &window_label,
        "https://api.openai.com/v1/chat/completions",
        "openai",
        &model,
        serde_json::json!({}),
        &history,
    )
    .await
}

#[tauri::command]
pub async fn ask_ai_anthropic(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    model: String,
    history: Vec<ChatTurnDto>,
) -> Result<String, String> {
    let api_key = secrets::read_api_key("anthropic")?;
    let model = model.trim();
    let img_b64 = get_crop_base64(&state, &window_label)?;

    // Chỉ đính ảnh vào lượt user GẦN NHẤT — xem giải thích chi tiết ở
    // ask_openai_compatible phía trên (tránh phình payload theo cấp số cộng
    // khi hội thoại dài).
    let last_user_idx = history.iter().rposition(|t| t.role == "user");
    let messages: Vec<serde_json::Value> = history
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            if turn.role == "user" && Some(i) == last_user_idx {
                serde_json::json!({
                    "role": "user",
                    "content": [
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": img_b64}},
                        {"type": "text", "text": turn.content},
                    ],
                })
            } else {
                serde_json::json!({"role": turn.role, "content": turn.content})
            }
        })
        .collect();

    // Anthropic không dùng message role "system" — system prompt là field riêng.
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1500,
        "stream": true,
        "system": SYSTEM_PROMPT,
        "messages": messages,
    });

    let client = &app.state::<HttpClientState>().client;
    let req = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key.as_str())
        .header("anthropic-version", "2023-06-01")
        .header("Accept", "text/event-stream")
        .json(&body);
    let resp = send_with_timeout(req).await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(friendly_error("Anthropic", status, &text));
    }

    let full = stream_sse(
        resp.bytes_stream(),
        |piece| {
            let _ = app.emit_to(&window_label, "ai:delta", DeltaPayload { piece: piece.to_string() });
        },
        |json| {
            if json["type"] == "content_block_delta" && json["delta"]["type"] == "text_delta" {
                json["delta"]["text"].as_str().map(|s| s.to_string())
            } else {
                None
            }
        },
    )
    .await?;

    Ok(finalize(full))
}

/// Google Gemini — định dạng request/response khác hẳn kiểu OpenAI:
/// - Role "assistant" ở Gemini gọi là "model".
/// - Ảnh gửi qua `inline_data` (base64) thay vì `image_url`.
/// - System prompt là field riêng `systemInstruction`.
/// - Endpoint dùng luôn tên model trong URL + `?alt=sse` để bật streaming SSE.
/// - Auth qua header `x-goog-api-key` (không phải Bearer).
/// Nơi lấy API key/endpoint gọi Gemini — 2 chế độ:
/// - `Backend { token }`: ĐÃ đăng nhập Google — gọi qua backend (giữ API key
///   thật, không cần người dùng tự có key). Đây là đường mặc định cho đại đa
///   số người dùng (học sinh/sinh viên/văn phòng), xem oauth.rs.
/// - `Direct { api_key }`: CHƯA đăng nhập — dùng API key người dùng tự nhập ở
///   Cài đặt (đường lùi cho người dùng nâng cao muốn tự quản lý key riêng).
enum GeminiAuth {
    Backend { token: String },
    Direct { api_key: String },
}

#[tauri::command]
pub async fn ask_ai_gemini(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    model: String,
    history: Vec<ChatTurnDto>,
) -> Result<String, String> {
    // Ưu tiên session đăng nhập Google nếu có — chỉ fallback về API key tự
    // nhập khi CHƯA đăng nhập (không phải khi đăng nhập lỗi tạm thời, vì
    // `read_session_token()` chỉ trả Err khi thật sự không có session nào).
    let auth = match crate::oauth::read_session_token() {
        Ok(token) => GeminiAuth::Backend { token },
        Err(_) => GeminiAuth::Direct { api_key: secrets::read_api_key("gemini")? },
    };
    let model = model.trim();
    let img_b64 = get_crop_base64(&state, &window_label)?;

    // Chỉ đính ảnh vào lượt user GẦN NHẤT — xem giải thích chi tiết ở
    // ask_openai_compatible phía trên (tránh phình payload theo cấp số cộng
    // khi hội thoại dài).
    let last_user_idx = history.iter().rposition(|t| t.role == "user");
    let contents: Vec<serde_json::Value> = history
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            let role = if turn.role == "assistant" { "model" } else { "user" };
            let mut parts = vec![serde_json::json!({"text": turn.content})];
            if turn.role == "user" && Some(i) == last_user_idx {
                parts.push(serde_json::json!({
                    "inline_data": {"mime_type": "image/png", "data": img_b64}
                }));
            }
            serde_json::json!({"role": role, "parts": parts})
        })
        .collect();

    let base_body = serde_json::json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": SYSTEM_PROMPT}]},
    });

    // Giảm "thinking" (suy luận ẩn trước khi trả lời, cộng thêm độ trễ) bằng
    // "thinkingLevel: minimal" — tham số MỚI của Gemini 3, thay thế
    // "thinkingBudget" của Gemini 2.5 (2 field không tương thích ngược, gửi
    // nhầm field cho model không hỗ trợ sẽ bị 400 "invalid argument" — đã gặp
    // thực tế với gemini-3.6-flash + thinkingBudget).
    //
    // KHÔNG đoán cứng theo tên model có hỗ trợ "minimal" hay không — theo bảng
    // hỗ trợ chính thức của Google, ngay trong CÙNG dòng Gemini 3, một số biến
    // thể (VD Gemini 3.7/3.8 Flash) lại KHÔNG hỗ trợ "minimal" và trả lỗi,
    // trong khi Gemini 3.5/3.6 Flash thì có — danh sách này có thể đổi theo
    // thời gian khi Google ra model mới. Thay vào đó: thử gửi kèm field này
    // trước, nếu bị 400 thì tự động gửi lại KHÔNG kèm field (fallback), không
    // cần cập nhật code mỗi khi có model Gemini mới.
    // Gọi qua backend thì KHÔNG biết chắc model server chọn có hỗ trợ
    // "thinkingLevel" hay không (model đó nằm trong cấu hình backend, app
    // không biết chính xác) — cứ thử, có sẵn cơ chế fallback-khi-400 bên dưới
    // rồi nên không sao. Gọi trực tiếp thì vẫn theo đúng tên model người dùng
    // chọn như cũ.
    let with_thinking = matches!(auth, GeminiAuth::Backend { .. }) || model.starts_with("gemini-3");
    let mut body = base_body.clone();
    if with_thinking {
        body["generationConfig"] = serde_json::json!({"thinkingConfig": {"thinkingLevel": "minimal"}});
    }

    let endpoint = match &auth {
        GeminiAuth::Backend { .. } => format!("{}/v1/gemini/stream", crate::oauth::backend_base_url()),
        GeminiAuth::Direct { .. } => {
            format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse")
        }
    };

    let client = &app.state::<HttpClientState>().client;
    let send = |body: &serde_json::Value| {
        let req = client.post(&endpoint).header("Accept", "text/event-stream").json(body);
        match &auth {
            GeminiAuth::Backend { token } => req.bearer_auth(token),
            GeminiAuth::Direct { api_key } => req.header("x-goog-api-key", api_key.as_str()),
        }
    };

    let mut resp = send_with_timeout(send(&body)).await?;
    if with_thinking && resp.status() == reqwest::StatusCode::BAD_REQUEST {
        eprintln!("[snip-ai][ai] Gemini từ chối thinkingLevel, thử lại không kèm field này");
        resp = send_with_timeout(send(&base_body)).await?;
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(friendly_error("Gemini", status, &text));
    }

    let full = stream_sse(
        resp.bytes_stream(),
        |piece| {
            let _ = app.emit_to(&window_label, "ai:delta", DeltaPayload { piece: piece.to_string() });
        },
        |json| json["candidates"][0]["content"]["parts"][0]["text"].as_str().map(|s| s.to_string()),
    )
    .await?;

    Ok(finalize(full))
}
