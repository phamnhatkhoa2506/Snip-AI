//! Gọi Google Gemini TRỰC TIẾP từ Rust bằng `reqwest`, thay vì qua
//! `@tauri-apps/plugin-http` (fetch ở JS). Lý do:
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
//! ⚠️ Bản v0 đầu tiên từng hỗ trợ CẢ NVIDIA NIM/OpenAI/Anthropic (người dùng tự
//! nhập API key riêng cho từng nơi) — đã BỎ HẲN khi chuyển sang đăng nhập
//! Google (xem oauth.rs): key thật quản lý ở backend, người dùng phổ thông
//! không cần biết/chọn provider gì cả. Chỉ còn Gemini.

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

/// System prompt — ép model trả lời gọn, có cấu trúc Markdown, và KHÔNG có
/// câu dẫn thừa ("Chắc chắn rồi, dưới đây là..."). Trước đây không có system
/// prompt nên output mỗi lần một kiểu, hay kèm lời dẫn và mô tả lại yêu cầu.
///
/// ⚠️ Đã cố tình bỏ cụm "vùng ảnh vừa cắt từ màn hình" trong bản trước — phát
/// hiện thực tế: model nhỏ (VD llama-3.2-11b-vision, hồi còn hỗ trợ NVIDIA)
/// khi không đọc được nội dung ảnh có xu hướng "mượn" luôn từ ngữ trong
/// system prompt để bịa ra câu trả lời nghe hợp lý (VD tự bịa đoạn text về
/// "vùng cắt ảnh" — đúng cụm từ lấy từ prompt — dù ảnh không hề nói về chủ đề
/// đó). Rút gọn tối đa + thêm quy tắc chống bịa rõ ràng để giảm rủi ro này.
const SYSTEM_PROMPT: &str = "\
Bạn là trợ lý AI phân tích ảnh/video, tích hợp trong app Snap AI.

QUY TẮC (bắt buộc):
1. CHỈ mô tả/trả lời dựa trên những gì THỰC SỰ thấy (video KHÔNG có âm thanh — không đoán/ \
bịa về âm thanh/lời thoại). Nếu ảnh/video mờ, quá nhỏ, hoặc không rõ nội dung, PHẢI nói \
thẳng điều đó (VD \"Ảnh quá mờ để đọc chữ\") — TUYỆT ĐỐI không bịa/đoán nội dung để có câu \
trả lời nghe hợp lý.
2. Vào thẳng nội dung, không mở đầu bằng \"Chắc chắn rồi\", \"Dưới đây là\", \"Trong ảnh/video \
này tôi thấy\" hay bất kỳ lời dẫn nào. Không nhắc lại yêu cầu của người dùng. Không thêm \
lời kết thừa kiểu \"Hy vọng giúp ích\".
3. Trả lời bằng tiếng Việt, TRỪ KHI người dùng yêu cầu ngôn ngữ khác, hoặc yêu cầu trích \
xuất/giữ nguyên văn bản gốc (khi đó giữ đúng ngôn ngữ gốc).
4. Dùng Markdown để cấu trúc câu trả lời: **in đậm** cho ý chính, danh sách gạch đầu dòng \
cho nhiều ý, `code` cho mã/lệnh/tên file/giá trị kỹ thuật, khối ```code``` cho đoạn mã \
nhiều dòng, bảng Markdown khi dữ liệu có dạng bảng.
5. Ngắn gọn, đúng trọng tâm — độ dài tương xứng với nội dung, không dài dòng.
6. Nếu ảnh/video hiện tại KHÔNG ĐỦ để trả lời chắc chắn (VD: đang xử lý sự cố \
nhiều bước, cần thấy bước tiếp theo/kết quả sau khi làm gì đó mới biết đúng-sai), hãy NÓI \
THẲNG điều đó và gợi ý người dùng chụp/quay thêm bước tiếp theo — chỉ cần nói bằng lời văn \
thường (VD \"Bạn thử chụp thêm màn hình sau khi bấm nút Chạy xem sao\"), KHÔNG cần định dạng \
đặc biệt gì.
7. Công thức toán (nếu có) viết bằng LaTeX, CHỈ dùng 1 trong 3 cách: `\\(...\\)` cho công \
thức ngắn trong dòng, hoặc `$$...$$`/`\\[...\\]` cho công thức riêng 1 dòng — TUYỆT ĐỐI \
KHÔNG dùng 1 dấu $ đơn lẻ (dễ lẫn với tiền tệ, VD \"$50\"). Phép nhân viết bằng dấu \\times \
hoặc \\cdot trong công thức, không dùng dấu *.
8. TỰ QUYẾT ĐỊNH vẽ sơ đồ khi nó giúp hiểu nhanh/rõ hơn hẳn so với chữ thuần — VD quy trình \
nhiều bước, mối quan hệ/luồng giữa các thành phần, cấu trúc phân cấp, trình tự thời gian, \
sơ đồ tư duy tổng hợp ý. KHÔNG cần người dùng yêu cầu rõ \"vẽ sơ đồ\" mới vẽ — nếu nội dung \
câu trả lời PHÙ HỢP để trực quan hoá thì cứ vẽ; ngược lại (câu hỏi đơn giản, chỉ 1-2 ý) thì \
KHÔNG vẽ, chữ thường vẫn tốt hơn. Khi vẽ, dùng ĐÚNG 1 khối mã ```mermaid với cú pháp Mermaid \
hợp lệ (flowchart TD/LR, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, gantt, \
mindmap, timeline...) — chọn loại sơ đồ khớp với nội dung. Có thể kèm cả lời giải thích bằng \
chữ THƯỜNG bên cạnh sơ đồ, không chỉ có mỗi sơ đồ trơ trọi.";

/// Chỉ dẫn thêm cho Gemini khi phiên đang hỏi là ẢNH (không áp dụng cho
/// video — 1 khung toạ độ không rõ "thuộc khung hình nào" trên video, để
/// dành xử lý sau). Đã kiểm chứng bằng test thực tế với ảnh giả lập kiểu
/// giao diện phần mềm (không phải ảnh đời thường như ví dụ trong tài liệu
/// Google): model trả toạ độ khớp gần như tuyệt đối so với vị trí thật, và
/// vẫn lấy được CÙNG LÚC với câu trả lời văn xuôi qua streamGenerateContent
/// bình thường — không cần tắt stream hay tách lệnh gọi riêng.
const GEMINI_BBOX_INSTRUCTION: &str = "\
\n\nNếu câu trả lời có nhắc đến 1 VỊ TRÍ/PHẦN TỬ CỤ THỂ trong ảnh (1 nút, 1 dòng chữ, 1 ô, \
1 vùng...), sau khi trả lời xong bằng lời, thêm CHÍNH XÁC 1 dòng JSON riêng ở CUỐI CÙNG \
(không nằm trong đoạn văn, không có chữ nào khác trên dòng đó):\n\
{\"box_2d\": [ymin, xmin, ymax, xmax]}\n\
Toạ độ chuẩn hoá theo thang 0-1000 so với kích thước ảnh. Nếu câu trả lời KHÔNG nhắc đến vị \
trí cụ thể nào (VD tóm tắt tổng quát, dịch toàn bộ văn bản, giải thích chung), KHÔNG thêm \
dòng JSON này.";

/// Chỉ dẫn thêm khi bật "Tra cứu web" (Google Search grounding). KHÔNG dặn
/// model tự viết "Nguồn: ..." vào câu trả lời nữa — đã thử ở bản đầu và gặp
/// thực tế: model có DÙNG search thật (tự nói "từ kết quả tìm kiếm cho
/// thấy...") nhưng KHÔNG chịu thêm dòng Nguồn như đã dặn, giống hệt bài học
/// từ mốc giờ video ("[mm:ss]" model cũng tự ý lệch định dạng đã dặn). Không
/// nên tin tưởng model tuân thủ 100% chỉ dẫn định dạng — lấy trích dẫn TRỰC
/// TIẾP từ `groundingMetadata` có cấu trúc trong response (xem `ask_ai_gemini`
/// bên dưới, nơi tự ghép "Nguồn: ..." vào cuối câu trả lời), chắc chắn hơn
/// nhiều so với hy vọng model tự giác.
const GEMINI_SEARCH_INSTRUCTION: &str =
    "\n\nBẠN CÓ THỂ tra cứu thông tin thật trên internet (giá cả, tin tức, thứ không có trong \
ảnh/video) khi câu hỏi cần đến. Không tra cứu nếu câu hỏi chỉ cần nhìn ảnh/video là trả lời được.";

/// Chỉ thêm khi phiên có tài liệu đính kèm (ảnh/PDF, xem attachments.rs) —
/// GIAI ĐOẠN 1 của tính năng "đính kèm file gốc", phục vụ trường hợp người
/// dùng chụp 1 vùng nhỏ nhưng muốn đưa thêm tài liệu gốc đầy đủ hơn làm ngữ
/// cảnh (VD chụp 1 biểu đồ trong báo cáo PDF, đính kèm luôn cả báo cáo).
const GEMINI_ATTACHMENT_INSTRUCTION: &str = "\
\n\nNgoài ảnh/video chính đang được hỏi, người dùng còn đính kèm thêm tài liệu tham khảo (ảnh/PDF, \
mỗi tệp có dòng \"Tệp đính kèm: <tên>\" ngay trước). Dùng các tệp này làm NGỮ CẢNH/DỮ LIỆU BỔ SUNG \
khi trả lời — đừng nhầm chúng là ảnh/video chính, và có thể gọi lại đúng tên tệp khi trích dẫn thông \
tin lấy từ đó.";

#[derive(Deserialize, Clone)]
pub struct ChatTurnDto {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

#[derive(Serialize, Clone)]
struct DeltaPayload {
    piece: String,
}

/// Đọc ảnh MỚI NHẤT của ĐÚNG phiên đang gọi (nhiều cửa sổ "Kết quả AI" có thể
/// mở cùng lúc, mỗi cửa sổ 1 ảnh riêng — xem giải thích ở
/// `AppState::crop_sessions`) — dùng cho `ask_ai_diagram` (tra cứu 1 lần cho
/// đúng ảnh đang xem, không cần cả chuỗi). Chat nhiều lượt/nhiều ảnh dùng
/// `get_media_chain_base64` bên dưới thay vì hàm này.
fn get_crop_base64(state: &State<'_, AppState>, window_label: &str) -> Result<String, String> {
    let sessions = state.crop_sessions.lock().unwrap();
    let list = sessions
        .get(window_label)
        .ok_or("Không tìm thấy ảnh cho phiên này (cửa sổ có thể đã bị đóng)")?;
    let bytes = list.last().ok_or("Phiên này chưa có ảnh nào")?;
    Ok(STANDARD.encode(bytes))
}

/// Giống `get_crop_base64` nhưng chấp nhận CẢ ẢNH LẪN VIDEO — 1 cửa sổ "Kết
/// quả AI" là phiên ảnh (snip) hoặc phiên video (quay màn hình), không bao
/// giờ cả hai, nên chỉ 1 trong 2 map (`crop_sessions`/`video_sessions`) có
/// entry khớp `window_label`. Trả về TOÀN BỘ chuỗi media của phiên (mỗi phần
/// tử: base64, mime_type), ĐÚNG THỨ TỰ đã chụp — dùng cho `ask_ai_gemini`
/// (chat nhiều lượt, có thể nhiều ảnh/video nhờ "+ Chụp thêm bước").
fn get_media_chain_base64(state: &State<'_, AppState>, window_label: &str) -> Result<Vec<(String, &'static str)>, String> {
    if let Some(list) = state.crop_sessions.lock().unwrap().get(window_label) {
        return Ok(list.iter().map(|b| (STANDARD.encode(b), "image/png")).collect());
    }
    if let Some(list) = state.video_sessions.lock().unwrap().get(window_label) {
        return Ok(list.iter().map(|b| (STANDARD.encode(b), "video/mp4")).collect());
    }
    Err("Không tìm thấy ảnh/video cho phiên này (cửa sổ có thể đã bị đóng)".into())
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
/// nguyên JSON thô lên UI. Thử lần lượt các field hay gặp; nếu không parse
/// được thì rơi về hiển thị đoạn text gốc (cắt bớt nếu quá dài) thay vì lỗi
/// trắng tay.
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

/// Google Gemini — định dạng request/response:
/// - Role "assistant" ở Gemini gọi là "model".
/// - Ảnh gửi qua `inline_data` (base64) thay vì `image_url`.
/// - System prompt là field riêng `systemInstruction`.
/// - Endpoint dùng luôn tên model trong URL + `?alt=sse` để bật streaming SSE.
/// - Auth qua header `x-goog-api-key` (không phải Bearer).
/// Nơi lấy API key/endpoint gọi Gemini — 2 chế độ:
/// - `Backend { token }`: ĐÃ đăng nhập Google — gọi qua backend (giữ API key
///   thật, không cần người dùng tự có key). Đây là đường DUY NHẤT cho người
///   dùng phổ thông (học sinh/sinh viên/văn phòng), xem oauth.rs.
/// - `Direct { api_key }`: CHƯA đăng nhập — dùng API key người dùng tự nhập.
///   ⚠️ Đường lùi này hiện KHÔNG CÒN CÁCH NÀO kích hoạt từ UI (màn hình nhập
///   API key đã gỡ hẳn cùng đợt bỏ NVIDIA/OpenAI/Anthropic — app bắt buộc
///   đăng nhập Google mới cho snip) — giữ lại trong code phòng trường hợp
///   sau này cần mở lại đường nâng cao này, không phải code chết vô nghĩa.
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
    // Nút "Hỏi thêm về vùng này" (khoanh vùng AI chỉ tới) — toạ độ chuẩn hoá
    // thang 0-1000, đúng định dạng box_2d [ymin,xmin,ymax,xmax]. `Option` nên
    // KHÔNG bắt buộc frontend phải truyền (mọi lượt hỏi bình thường không có
    // field này, hành vi giữ nguyên như cũ).
    region: Option<[u32; 4]>,
    // Bật "Google Search grounding" — cho Gemini tự tra cứu thông tin THẬT
    // ngoài internet (giá cả, tin tức, thứ không có trong ảnh/video) khi cần,
    // kèm trích nguồn. `Option` để không bắt buộc frontend phải truyền.
    search: Option<bool>,
) -> Result<String, String> {
    let use_search = search.unwrap_or(false);
    // Ưu tiên session đăng nhập Google nếu có — chỉ fallback về API key tự
    // nhập khi CHƯA đăng nhập (không phải khi đăng nhập lỗi tạm thời, vì
    // `read_session_token()` chỉ trả Err khi thật sự không có session nào).
    let auth = match crate::oauth::read_session_token() {
        Ok(token) => GeminiAuth::Backend { token },
        Err(_) => GeminiAuth::Direct { api_key: secrets::read_api_key("gemini")? },
    };
    let model = model.trim();
    // Chấp nhận cả ảnh (snip) lẫn video (quay màn hình) — 1 cửa sổ chỉ là 1
    // trong 2, `get_media_chain_base64` tự tìm đúng loại và trả về TOÀN BỘ
    // chuỗi (1 phiên có thể có nhiều ảnh/video nhờ "+ Chụp thêm bước", xem
    // AppState::crop_sessions), đúng thứ tự đã chụp.
    let mut media_chain = get_media_chain_base64(&state, &window_label)?;
    if media_chain.is_empty() {
        return Err("Phiên này chưa có ảnh/video nào".into());
    }
    let mime_type = media_chain[0].1;
    // Tài liệu đính kèm THÊM (ảnh/PDF, xem attachments.rs) — hoàn toàn TÙY
    // CHỌN, phiên nào không đính gì thì đây luôn là mảng rỗng.
    let attachments = crate::attachments::get_attachment_chain_base64(&state, &window_label);

    // Có `region` VÀ đang là ảnh (không áp dụng cho video) -> cắt tạm đúng
    // vùng đó để gửi CHO LƯỢT NÀY, không đụng gì tới ảnh gốc lưu trong
    // `crop_sessions` (các câu hỏi khác trong cùng phiên vẫn thấy toàn ảnh).
    // Áp dụng cho ảnh MỚI NHẤT trong chuỗi (khung khoanh vùng luôn vẽ theo
    // ảnh mới nhất, xem GEMINI_BBOX_INSTRUCTION + result/+page.svelte).
    if let Some([ymin, xmin, ymax, xmax]) = region {
        if mime_type.starts_with("image/") {
            if let Some(last) = media_chain.last_mut() {
                let raw = STANDARD.decode(&last.0).map_err(|e| format!("Lỗi giải mã ảnh: {e}"))?;
                let cropped = crate::capture::crop_by_normalized_box(&raw, ymin, xmin, ymax, xmax)?;
                last.0 = STANDARD.encode(cropped);
            }
        }
    }

    // Chỉ đính ảnh/video vào lượt user GẦN NHẤT — tránh phình payload theo
    // cấp số cộng khi hội thoại dài (trước đây gửi lại ảnh ở TẤT CẢ các lượt
    // user để giữ "grounding" cho model, nhưng làm payload phình to dần: hỏi
    // lần 4 gửi lại y hệt ảnh đó 4 lần trong 1 request). Đính CẢ CHUỖI (không
    // chỉ 1 media) vào ĐÚNG lượt đó — mỗi ảnh/video 1 "inline_data" riêng,
    // Gemini tự hiểu đây là nhiều tấm ảnh/nhiều đoạn video liên quan tới cùng
    // 1 câu hỏi.
    let last_user_idx = history.iter().rposition(|t| t.role == "user");
    let contents: Vec<serde_json::Value> = history
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            let role = if turn.role == "assistant" { "model" } else { "user" };
            let mut parts = vec![serde_json::json!({"text": turn.content})];
            if turn.role == "user" && Some(i) == last_user_idx {
                for (b64, mime) in &media_chain {
                    parts.push(serde_json::json!({
                        "inline_data": {"mime_type": mime, "data": b64}
                    }));
                }
                // Mỗi file đính kèm kèm 1 dòng text ghi rõ TÊN FILE ngay trước
                // — giúp model phân biệt/nhắc lại đúng tên khi có NHIỀU file
                // đính kèm cùng lúc, thay vì chỉ thấy 1 khối inline_data trần
                // không rõ là tài liệu nào.
                for (b64, mime, name) in &attachments {
                    parts.push(serde_json::json!({"text": format!("Tệp đính kèm: {name}")}));
                    parts.push(serde_json::json!({
                        "inline_data": {"mime_type": mime, "data": b64}
                    }));
                }
            }
            serde_json::json!({"role": role, "parts": parts})
        })
        .collect();

    // Lượt hỏi có `region` (đã cắt ảnh cho ĐÚNG lượt này) HOẶC phiên có TỪ 2
    // ẢNH TRỞ LÊN thì KHÔNG kèm chỉ dẫn box_2d nữa: toạ độ model trả về lúc
    // này sẽ tính theo ẢNH ĐÃ CẮT (khác hệ quy chiếu với ảnh gốc đang hiển
    // thị), còn với chuỗi nhiều ảnh thì KHÔNG RÕ toạ độ trả về thuộc về ẢNH
    // NÀO trong chuỗi — cả 2 trường hợp frontend đều có thể vẽ khung sai chỗ
    // nếu không chặn. Đơn giản hoá: chỉ bật box_2d khi phiên có ĐÚNG 1 ảnh.
    let mut system_text = if mime_type.starts_with("image/") && region.is_none() && media_chain.len() <= 1 {
        format!("{SYSTEM_PROMPT}{GEMINI_BBOX_INSTRUCTION}")
    } else {
        SYSTEM_PROMPT.to_string()
    };
    if use_search {
        system_text.push_str(GEMINI_SEARCH_INSTRUCTION);
    }
    if !attachments.is_empty() {
        system_text.push_str(GEMINI_ATTACHMENT_INSTRUCTION);
    }
    let mut base_body = serde_json::json!({
        "contents": contents,
        "systemInstruction": {"parts": [{"text": system_text}]},
    });

    // Bật Google Search grounding. KHÔNG chắc chắn tên field đúng — tài liệu
    // Google mô tả kỹ cho API "Interactions" mới, còn app dùng endpoint
    // generateContent cổ điển thì không tìm được ví dụ chính thức. Thử
    // "googleSearch" (khớp quy ước camelCase của các field cấp cao khác app
    // đang dùng thành công: systemInstruction, generationConfig) trước; có
    // cơ chế tự đổi sang "google_search" nếu bị 400 (xem bên dưới, cùng kỹ
    // thuật đã dùng cho thinkingLevel).
    if use_search {
        base_body["tools"] = serde_json::json!([{"googleSearch": {}}]);
    }

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
    if use_search && resp.status() == reqwest::StatusCode::BAD_REQUEST {
        // "googleSearch" (thử ở trên) bị từ chối -> đổi sang "google_search",
        // bỏ luôn thinkingConfig cho lần thử cuối này (đơn giản hoá, không cần
        // tổ hợp cả 4 khả năng thinking x tools).
        eprintln!("[snip-ai][ai] Gemini từ chối tools=googleSearch, thử lại với google_search");
        let mut retry_body = base_body.clone();
        retry_body["tools"] = serde_json::json!([{"google_search": {}}]);
        resp = send_with_timeout(send(&retry_body)).await?;
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let via = match &auth {
            GeminiAuth::Backend { .. } => "backend",
            GeminiAuth::Direct { .. } => "API key trực tiếp",
        };
        eprintln!("[snip-ai][ai] Gemini lỗi HTTP {status} (qua {via}): {text}");

        // Lỗi chặn theo VÙNG ĐỊA LÝ — thông báo gốc của Google ("User location
        // is not supported for the API use") khiến người dùng tưởng MÁY MÌNH ở
        // vùng bị cấm và loay hoay bật VPN, trong khi thật ra Google đang xét
        // IP của BÊN GỌI: khi đã đăng nhập thì bên gọi là Cloudflare Worker,
        // không phải máy người dùng. Worker có thể bị route sang colo nằm
        // trong vùng Google chặn -> lỗi này xuất hiện lúc có lúc không dù
        // người dùng ngồi yên một chỗ. Dịch lại cho đúng bản chất.
        if text.contains("User location is not supported") {
            return Err(match auth {
                GeminiAuth::Backend { .. } => "Máy chủ trung gian đang bị Google chặn theo vùng (không phải do máy hoặc mạng của bạn). Thử lại sau ít phút — sự cố này thường tự hết khi request được định tuyến lại."
                    .to_string(),
                GeminiAuth::Direct { .. } => "Google không hỗ trợ Gemini API ở vị trí mạng hiện tại của bạn. Thử đăng nhập bằng Google để dùng máy chủ của app thay cho API key riêng."
                    .to_string(),
            });
        }

        return Err(friendly_error("Gemini", status, &text));
    }

    // Thu thập trích dẫn (nếu bật search) NGAY TRONG LÚC đọc từng mẩu chunk —
    // groundingMetadata thường chỉ xuất hiện ở 1-2 chunk cuối (lúc model kết
    // thúc câu trả lời), không rải đều mọi chunk. Dùng Mutex (không phải
    // RefCell) vì closure `extract` bắt buộc là `Fn` (không phải `FnMut`) —
    // đọc/ghi qua Mutex vẫn hợp lệ với `Fn` nhờ tính chất "interior mutability".
    let citations: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>> = Default::default();
    let citations_for_extract = citations.clone();

    let full = stream_sse(
        resp.bytes_stream(),
        |piece| {
            let _ = app.emit_to(&window_label, "ai:delta", DeltaPayload { piece: piece.to_string() });
        },
        move |json| {
            if use_search {
                if let Some(chunks) = json["candidates"][0]["groundingMetadata"]["groundingChunks"].as_array() {
                    let mut list = citations_for_extract.lock().unwrap();
                    for c in chunks {
                        let (Some(uri), Some(title)) = (c["web"]["uri"].as_str(), c["web"]["title"].as_str()) else {
                            continue;
                        };
                        let pair = (title.to_string(), uri.to_string());
                        if !list.contains(&pair) {
                            list.push(pair);
                        }
                    }
                }
            }
            json["candidates"][0]["content"]["parts"][0]["text"].as_str().map(|s| s.to_string())
        },
    )
    .await?;

    let mut answer = finalize(full);
    // Trích dẫn lấy TRỰC TIẾP từ dữ liệu response, không phụ thuộc model có
    // tự viết "Nguồn: ..." vào câu trả lời hay không (xem giải thích ở
    // GEMINI_SEARCH_INSTRUCTION) — chắc chắn hơn hẳn.
    let cites = citations.lock().unwrap();
    if !cites.is_empty() {
        let list = cites.iter().map(|(title, uri)| format!("[{title}]({uri})")).collect::<Vec<_>>().join(", ");
        answer.push_str(&format!("\n\nNguồn: {list}"));
    }
    drop(cites);

    Ok(answer)
}

// ── "Sơ đồ từ vựng" — dịch ảnh thành 1 mạng liên kết từ vựng, KHÁC HẲN chip
// "Dịch" (dịch phẳng nguyên đoạn văn). Dùng `response_schema` ép cấu trúc
// JSON — đã kiểm chứng thực nghiệm bằng binary test tạm: hoạt động ĐÚNG cả
// khi kèm ảnh qua `inline_data` LẪN qua streamGenerateContent?alt=sse, nên
// tái dùng được y hệt đường gọi/luồng auth của ask_ai_gemini, không cần
// thêm route backend mới. Chỉ áp dụng cho ẢNH — khái niệm "từ vựng trong
// ảnh" rõ nghĩa, còn "từ vựng trong video" mơ hồ (từ nào, xuất hiện lúc nào)
// nên KHÔNG bật cho phiên video (giống chip "Mã / Lỗi" không có bản video).
const DIAGRAM_PROMPT: &str = "\
Đọc từ/cụm từ chính xuất hiện trong ảnh (thường là 1 từ vựng nổi bật, có thể kèm ngữ cảnh câu). \
Xác định nghĩa của nó và các từ LIÊN QUAN thật sự hữu ích để học (đồng nghĩa, trái nghĩa, hoặc \
từ cùng nhóm nghĩa/thường đi cùng) — 3 đến 5 từ liên quan. Với MỖI từ liên quan, cho 1 câu ví dụ \
ngắn, tự nhiên, dễ hiểu.";

#[derive(Serialize, Deserialize, Clone)]
pub struct DiagramTerm {
    pub term: String,
    pub translation: String,
    /// VD "synonym", "antonym", "related concept" — hiển thị lại làm nhãn
    /// đường nối trên sơ đồ, không dịch cứng thành enum để model tự do diễn
    /// đạt quan hệ chính xác hơn (thử enum cố định dễ ép model chọn sai loại
    /// gần đúng nhất thay vì đúng nhất).
    pub relation: String,
    pub example: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiagramData {
    pub main_term: String,
    pub translation: String,
    pub related: Vec<DiagramTerm>,
}

fn diagram_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "OBJECT",
        "properties": {
            "mainTerm": {"type": "STRING"},
            "translation": {"type": "STRING"},
            "related": {
                "type": "ARRAY",
                "minItems": 3,
                "maxItems": 5,
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "term": {"type": "STRING"},
                        "translation": {"type": "STRING"},
                        "relation": {"type": "STRING"},
                        "example": {"type": "STRING"}
                    },
                    "required": ["term", "translation", "relation", "example"]
                }
            }
        },
        "required": ["mainTerm", "translation", "related"]
    })
}

#[tauri::command]
pub async fn ask_ai_diagram(app: AppHandle, state: State<'_, AppState>, window_label: String, model: String) -> Result<DiagramData, String> {
    let auth = match crate::oauth::read_session_token() {
        Ok(token) => GeminiAuth::Backend { token },
        Err(_) => GeminiAuth::Direct { api_key: secrets::read_api_key("gemini")? },
    };
    let model = model.trim();

    // Chỉ ảnh MỚI NHẤT trong chuỗi — sơ đồ từ vựng là tra cứu 1-lần cho 1
    // từ/cụm từ cụ thể đang thấy, không phải hội thoại nhiều lượt, nên không
    // cần cả chuỗi media như ask_ai_gemini.
    let img_b64 = get_crop_base64(&state, &window_label)?;

    let body = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [
                {"inline_data": {"mime_type": "image/png", "data": img_b64}},
                {"text": DIAGRAM_PROMPT},
            ],
        }],
        "systemInstruction": {"parts": [{"text": SYSTEM_PROMPT}]},
        "generationConfig": {
            "response_mime_type": "application/json",
            "response_schema": diagram_schema(),
        },
    });

    let endpoint = match &auth {
        GeminiAuth::Backend { .. } => format!("{}/v1/gemini/stream", crate::oauth::backend_base_url()),
        GeminiAuth::Direct { .. } => {
            format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse")
        }
    };

    let client = &app.state::<HttpClientState>().client;
    let req = client.post(&endpoint).header("Accept", "text/event-stream").json(&body);
    let req = match &auth {
        GeminiAuth::Backend { token } => req.bearer_auth(token),
        GeminiAuth::Direct { api_key } => req.header("x-goog-api-key", api_key.as_str()),
    };
    let resp = send_with_timeout(req).await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        eprintln!("[snip-ai][ai] Gemini (sơ đồ từ vựng) lỗi HTTP {status}: {text}");
        return Err(friendly_error("Gemini", status, &text));
    }

    // Không cần "reveal" theo mẩu nhỏ như câu trả lời văn xuôi — JSON dở dang
    // giữa chừng không có gì để hiển thị hợp lý, nên bỏ qua on_piece (no-op),
    // chỉ lấy full text sau khi stream đọc xong.
    let full = stream_sse(
        resp.bytes_stream(),
        |_piece| {},
        |json| json["candidates"][0]["content"]["parts"][0]["text"].as_str().map(|s| s.to_string()),
    )
    .await?;

    serde_json::from_str::<DiagramData>(full.trim())
        .map_err(|e| format!("AI trả về dữ liệu không đúng cấu trúc mong đợi ({e}). Thử lại xem sao."))
}
