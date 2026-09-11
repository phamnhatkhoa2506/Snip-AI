<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { LogicalSize } from "@tauri-apps/api/dpi";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "$lib/Icon.svelte";
  import ScrollArea from "$lib/ScrollArea.svelte";
  import BoxedImage from "$lib/BoxedImage.svelte";
  import { QUICK_PROMPTS, VIDEO_PROMPTS, PROMPT_EXPLAIN, PROMPT_VIDEO_EXPLAIN, type QuickPrompt } from "$lib/config";
  import { currentModel, loadSettings, type Settings } from "$lib/settings";
  import { askAIStream, type ChatTurn } from "$lib/aiClient";
  import { renderMarkdown, markdownToPlainText, linkifyTimestamps } from "$lib/markdown";

  type Phase = "ask" | "chat";

  // Cửa sổ này dùng chung 1 route cho CẢ 2 loại phiên: snip ảnh (label
  // "result-N") và quay video (label "record-N") — tự nhận biết qua tiền tố
  // label, xem commands.rs (RESULT_LABEL_PREFIX/RECORD_LABEL_PREFIX). Khác
  // ảnh: cửa sổ video chỉ được TẠO SAU KHI quay xong (xem
  // record.rs::start_recording) — video LUÔN đã có sẵn trong `video_sessions`
  // ngay từ lúc cửa sổ này mount, không cần chờ/nghe event gì thêm.
  const isVideoSession = getCurrentWindow().label.startsWith("record-");

  // Bộ chip gợi ý khác nhau giữa 2 chế độ. Không chỉ là đổi chữ "ảnh" thành
  // "video": video có trục thời gian và bị Gemini lấy mẫu thưa (~1 khung/giây)
  // nên câu hỏi phải đặt khác hẳn — xem giải thích đầy đủ ở config.ts, ngay
  // trên PROMPT_VIDEO_OCR.
  const quickPrompts = isVideoSession ? VIDEO_PROMPTS : QUICK_PROMPTS;

  let phase = $state<Phase>("ask");
  /** Toàn bộ chuỗi ảnh/video của phiên này, ĐÚNG THỨ TỰ đã chụp — 1 phiên có
   * thể có NHIỀU media nhờ "+ Chụp thêm bước" (chuỗi snip có dẫn dắt, xem
   * triggerAppendCapture bên dưới), không chỉ 1 như trước. */
  let mediaChain = $state<string[]>([]);
  /** Base64 của media MỚI NHẤT trong chuỗi — dùng cho mọi chỗ trước đây chỉ
   * biết "1 ảnh/video" (avatar thu nhỏ, disabled-state của input...). Tên
   * giữ nguyên "mediaB64" để không phải đổi lại toàn bộ chỗ dùng cũ. */
  let mediaB64 = $derived(mediaChain[mediaChain.length - 1] ?? "");
  let question = $state("");
  let history = $state<ChatTurn[]>([]);
  let busy = $state(false);
  let followupText = $state("");
  let followupInputEl = $state<HTMLInputElement | null>(null);
  let copyFlash = $state(false);
  let error = $state("");
  let statusLine = $state("");
  let elapsedSec = $state(0);
  let modelLabel = $state("");
  let showImagePreview = $state(false);

  // ── Chỉ định giây/khoảng để hỏi (chỉ áp dụng cho phiên VIDEO) ───────────
  // Không cắt file video thật (đã kiểm chứng field `video_metadata` của
  // Gemini bị model mới lờ đi — xem ghi chú trong ai.rs). Thay vào đó gửi
  // NGUYÊN video như cũ, chỉ thêm 1 câu chỉ dẫn bằng lời vào cuối câu hỏi.
  // Đã kiểm chứng thực tế: hỏi thẳng "tại giây thứ N có gì" trên video KHÔNG
  // cắt vẫn cho Gemini trả lời đúng tuyệt đối (model tự gắn timestamp theo
  // từng khung lấy mẫu) — rẻ hơn nhiều so với tự dựng bộ cắt MP4 mà vẫn đáng
  // tin cậy ngang nhau.
  let videoEl = $state<HTMLVideoElement | null>(null);
  let videoDuration = $state(0);
  let rangeStart = $state(0);
  let rangeEnd = $state(0);
  /** false = chưa động vào thanh chọn -> không giới hạn gì, hỏi cả video. */
  let rangeTouched = $state(false);
  /** Thanh kéo chọn khoảng CHỈ hiện mặc định lúc mới mở (`phase === "ask"`)
   * — lúc đang chat thì ẩn theo mặc định (tiết kiệm chỗ cho khung hội thoại),
   * bấm nút "Chọn khoảng thời gian" mới hiện ra. */
  let showRangeSliderInChat = $state(false);

  function formatClock(totalSec: number): string {
    const s = Math.max(0, Math.round(totalSec));
    const m = Math.floor(s / 60);
    const ss = String(s % 60).padStart(2, "0");
    return `${m}:${ss}`;
  }

  /** true nếu 2 tay cầm gần như trùng nhau -> coi là "1 thời điểm" thay vì
   * "1 khoảng" (chênh dưới 1s, khó kéo trùng tuyệt đối bằng chuột). */
  const isTimePoint = $derived(rangeEnd - rangeStart < 1);

  const timeRangeLabel = $derived(
    isTimePoint ? formatClock(rangeStart) : `${formatClock(rangeStart)}–${formatClock(rangeEnd)}`,
  );

  /** Câu chỉ dẫn nối vào CUỐI nội dung thật gửi cho AI (không hiện lên bong
   * bóng chat) khi người dùng đã chỉ định thời điểm/khoảng. Rỗng nếu chưa
   * chỉnh gì -> hành vi giữ nguyên như trước (hỏi cả video). */
  const timeContextSuffix = $derived(
    !rangeTouched
      ? ""
      : isTimePoint
        ? `\n\n(Chỉ tập trung vào đúng thời điểm ${formatClock(rangeStart)} trong video, không phải toàn bộ video.)`
        : `\n\n(Chỉ tập trung vào đoạn video từ ${formatClock(rangeStart)} đến ${formatClock(rangeEnd)}, không phải toàn bộ video.)`,
  );

  function onVideoLoadedMetadata() {
    if (!videoEl || !Number.isFinite(videoEl.duration)) return;
    videoDuration = videoEl.duration;
    rangeEnd = videoEl.duration;
  }

  /** Kéo tay cầm thì tua luôn video xem trước đúng thời điểm đó — cho người
   * dùng thấy ngay "giây này có gì" thay vì phải đoán bằng số giây suông.
   * Cập nhật CẢ HAI ref video — `videoEl` (video lớn lúc mới mở, phase=ask)
   * lẫn `modalVideoEl` (video trong ảnh phóng to lúc đang chat) — 2 phase
   * không bao giờ cùng hiện 1 lúc nên chỉ đúng 1 trong 2 tồn tại tại 1 thời
   * điểm, cập nhật cả 2 vô hại và đỡ phải biết trước đang ở ngữ cảnh nào. */
  function seekPreview(t: number) {
    if (videoEl) videoEl.currentTime = t;
    if (modalVideoEl) modalVideoEl.currentTime = t;
  }

  // ── Mốc giờ bấm được trong câu trả lời (xem linkifyTimestamps) — bấm 1 mốc
  // thì MỞ ảnh phóng to (video lúc đó chỉ còn là avatar 32px trong header,
  // không đủ để xem) và tự tua tới đúng giây đó. -----------------------------
  /** Giây cần tua tới NGAY KHI video trong modal sẵn sàng — modal luôn tạo
   * MỚI thẻ <video> mỗi lần mở (không tái dùng), nên phải đợi `loadedmetadata`
   * của lần mở NÀY rồi mới set `currentTime`, set sớm hơn sẽ vô tác dụng. */
  let pendingSeekTime = $state<number | null>(null);
  let modalVideoEl = $state<HTMLVideoElement | null>(null);

  /** 1 listener DUY NHẤT cho cả bong bóng trả lời — xử lý cả mốc giờ video
   * (`.ts-link`) lẫn link nguồn trích dẫn (`<a href>` từ "Nguồn: ..." khi
   * bật tra cứu web, xem GEMINI_SEARCH_INSTRUCTION trong ai.rs). */
  function handleAnswerClick(e: MouseEvent) {
    const target = e.target as HTMLElement;

    const tsBtn = target.closest<HTMLElement>(".ts-link");
    if (tsBtn) {
      const ts = Number(tsBtn.dataset.ts);
      if (Number.isFinite(ts)) {
        pendingSeekTime = ts;
        openMediaPreview();
      }
      return;
    }

    // Link trong câu trả lời (VD nguồn trích dẫn) — BẮT BUỘC chặn hành vi
    // mặc định rồi tự mở bằng trình duyệt HỆ THỐNG. Không chặn thì WebView2
    // sẽ ĐIỀU HƯỚNG NGUYÊN CỬA SỔ APP sang trang đó (thay hẳn giao diện
    // "Kết quả AI" bằng trang web, không có cách nào quay lại) — lỗi thực tế
    // đã gặp: bấm vào 1 link nguồn, mất luôn cửa sổ, phải đóng app lại.
    const link = target.closest<HTMLAnchorElement>("a[href]");
    if (link) {
      e.preventDefault();
      openUrl(link.href).catch((err) => console.warn("[snip-ai] Không mở được link:", err));
    }
  }

  // ── "+ Chụp thêm bước" (chuỗi snip có dẫn dắt) ─────────────────────────
  // AI có thể GỢI Ý bằng lời văn thường ("bạn thử chụp thêm bước tiếp theo
  // xem sao") — KHÔNG cần model phát ra tín hiệu máy đọc được gì cả (đã rút
  // kinh nghiệm từ box_2d/mốc giờ video/trích nguồn: không thể tin model
  // tuân thủ 100% 1 định dạng đặc biệt). Nút này LUÔN có sẵn, người đọc gợi ý
  // của AI rồi tự bấm — vẫn đúng tinh thần agentic (AI đề nghị) +
  // human-in-the-loop (người quyết định), chỉ là không đặt cược vào việc
  // model tuân thủ giao thức.
  let appendCaptureBusy = $state(false);
  async function triggerAppendCapture() {
    appendCaptureBusy = true;
    try {
      await invoke(isVideoSession ? "trigger_recording_for_session" : "trigger_capture_for_session", {
        windowLabel: getCurrentWindow().label,
      });
    } catch (e) {
      error = String(e);
    } finally {
      appendCaptureBusy = false;
    }
  }

  /** Đang xem MEDIA NÀO trong chuỗi ở ảnh phóng to — `null` = luôn bám theo
   * media MỚI NHẤT (mặc định). Bấm vào 1 thumbnail cụ thể trong dải mới ghim
   * cố định vào đúng cái đó. */
  let previewIndex = $state<number | null>(null);
  const previewMediaB64 = $derived(mediaChain[previewIndex ?? mediaChain.length - 1] ?? "");

  function onModalVideoReady() {
    if (modalVideoEl && pendingSeekTime != null) {
      modalVideoEl.currentTime = pendingSeekTime;
      pendingSeekTime = null;
    }
  }

  function clearTimeRange() {
    rangeTouched = false;
    rangeStart = 0;
    rangeEnd = videoDuration;
  }

  /** Cửa sổ "Kết quả AI" mặc định khá nhỏ (480x340, vừa đủ cho khung chat
   * gọn) — nhưng ảnh lớn + dải thumbnail nhiều bước (chuỗi snip) thì chật.
   * Mở ảnh/video phóng to LUÔN kèm việc đảm bảo cửa sổ đủ rộng để xem thoải
   * mái. CHỈ PHÌNH TO, không bao giờ tự thu nhỏ lại — tôn trọng nếu người
   * dùng đã tự kéo cửa sổ nhỏ hơn theo ý mình trước đó. */
  const COMFORTABLE_PREVIEW_SIZE = { width: 640, height: 620 };
  async function ensureRoomyWindow() {
    try {
      const win = getCurrentWindow();
      const scale = await win.scaleFactor();
      const current = (await win.innerSize()).toLogical(scale);
      if (current.width < COMFORTABLE_PREVIEW_SIZE.width || current.height < COMFORTABLE_PREVIEW_SIZE.height) {
        await win.setSize(
          new LogicalSize(
            Math.max(current.width, COMFORTABLE_PREVIEW_SIZE.width),
            Math.max(current.height, COMFORTABLE_PREVIEW_SIZE.height),
          ),
        );
      }
    } catch (e) {
      console.warn("[snip-ai] Không tự phình được cửa sổ:", e);
    }
  }

  function openMediaPreview() {
    showImagePreview = true;
    ensureRoomyWindow();
  }

  /** Mở ảnh phóng to KÈM thanh kéo bên trong — dùng lúc đang chat (chọn/
   * chỉnh khoảng thời gian). Xem lại được video trong lúc kéo, thay vì kéo
   * "mù" ngay trong khung hội thoại chật hẹp. */
  function openRangeSliderModal() {
    showRangeSliderInChat = true;
    openMediaPreview();
  }

  /** Đóng ảnh phóng to — LUÔN dọn kèm `showRangeSliderInChat` dù đóng bằng
   * đường nào (bấm X, bấm ra ngoài, Esc), không chỉ nút "Xong" trong thanh
   * kéo. Thiếu bước này thì lần mở modal SAU (VD chỉ để xem lại video, không
   * liên quan tới chọn khoảng) sẽ bị dính hiện luôn thanh kéo từ lần trước. */
  function closeImagePreview() {
    showImagePreview = false;
    showRangeSliderInChat = false;
    previewIndex = null;
  }

  // Hiệu ứng "reveal" chữ giống ChatGPT/Claude: tách từng đoạn AI trả về thành
  // các mẩu nhỏ (từ + khoảng trắng), mỗi mẩu fade-in riêng thay vì bật cả cục.
  interface Chunk {
    id: number;
    text: string;
  }
  let streamChunks = $state<Chunk[]>([]);
  let chunkIdSeq = 0;

  function pushStreamPiece(piece: string) {
    const parts = piece.match(/\S+\s*|\s+/g) ?? [piece];
    for (const p of parts) streamChunks.push({ id: chunkIdSeq++, text: p });
  }

  let transcriptEl = $state<HTMLDivElement | null>(null);

  /** `silent`: KHÔNG hiện lỗi nếu ảnh/video chưa có — dùng cho lần thử đầu
   * tiên lúc mới mount, vì cửa sổ này được mở NGAY (trước khi ảnh/video xử
   * lý xong) để phản hồi tức thì, nên có thể CHƯA kịp nạp vào
   * `crop_sessions`/`video_sessions` phía Rust — đó là chuyện bình thường,
   * không phải lỗi. Sẽ tự nạp lại khi nhận event "ai:crop-ready"/
   * "recording:ready" (xem onMount bên dưới). */
  async function loadMediaChain(silent: boolean) {
    try {
      mediaChain = await invoke<string[]>(isVideoSession ? "get_recording_chain_base64" : "get_crop_chain_base64", {
        windowLabel: getCurrentWindow().label,
      });
    } catch (e) {
      if (!silent) error = String(e);
    }
  }

  onMount(() => {
    // Cửa sổ này luôn được TẠO MỚI mỗi lần snip/quay (xem commands.rs), nên
    // onMount chạy fresh mỗi lần — không cần lắng nghe event reset.
    const unlistens: (() => void)[] = [];
    const s = loadSettings();
    modelLabel = currentModel(s);

    if (isVideoSession) {
      // Phiên video: cửa sổ chỉ mở SAU KHI quay xong, video đã sẵn sàng ngay
      // từ đầu — nạp thẳng, không cần silent/event gì cả.
      loadMediaChain(false);
    } else {
      // Phiên ảnh: cửa sổ mở NGAY khi vừa chọn xong vùng (trước khi crop/resize
      // ảnh xong) để phản hồi tức thì thay vì "chờ mù" — nên thử nạp ảnh ngay
      // (silent, phòng trường hợp ảnh đã kịp xử lý xong), đồng thời lắng nghe
      // event "ai:crop-ready" từ Rust để nạp lại khi ảnh THẬT SỰ sẵn sàng.
      loadMediaChain(true);
      listen("ai:crop-ready", () => loadMediaChain(false)).then((fn) => unlistens.push(fn));
    }

    // "+ Chụp thêm bước" đã thêm xong 1 ảnh/video vào chuỗi (xem
    // commands.rs::append_capture_to_session / record.rs) — nạp lại chuỗi.
    listen("ai:chain-updated", () => loadMediaChain(false)).then((fn) => unlistens.push(fn));

    return () => unlistens.forEach((fn) => fn());
  });

  /** Lưu lịch sử SAU MỖI lượt AI trả lời thành công — lần gọi đầu của cửa sổ
   * này tạo bản ghi mới (Rust tự copy ảnh/video từ AppState ra đĩa), các lần
   * sau chỉ cập nhật lại `turns`. Cố tình KHÔNG `await`/không chặn UI và nuốt
   * lỗi im lặng — đây là tính năng PHỤ, không được làm hỏng luồng hỏi-đáp
   * chính nếu lỡ ghi đĩa thất bại (hết dung lượng, quyền file, v.v.). */
  function saveHistoryTurn(settings: Settings) {
    invoke("history_save_turn", {
      windowLabel: getCurrentWindow().label,
      model: currentModel(settings),
      turns: history,
    }).catch((e) => console.warn("[snip-ai] Lưu lịch sử thất bại (bỏ qua):", e));
  }

  async function scrollToBottom() {
    await tick();
    transcriptEl?.scrollTo({ top: transcriptEl.scrollHeight, behavior: "smooth" });
  }

  // ── Khoanh vùng AI chỉ tới (chỉ phiên ẢNH) — Rust dặn Gemini thêm 1 dòng
  // JSON `{"box_2d":[ymin,xmin,ymax,xmax]}` ở CUỐI câu trả lời khi có nhắc
  // tới 1 vị trí cụ thể (xem GEMINI_BBOX_INSTRUCTION trong ai.rs). Bóc dòng
  // đó ra khỏi văn bản HIỂN THỊ ở đây — người dùng không cần thấy JSON thô,
  // chỉ cần thấy khung vẽ trên ảnh (xem $lib/BoxedImage.svelte — nơi thật sự
  // quy đổi toạ độ 0-1000 sang pixel, tuỳ theo kích thước hiển thị).
  type Box2d = [number, number, number, number]; // [ymin, xmin, ymax, xmax]

  function extractBoxFromAnswer(text: string): { text: string; box: Box2d | null } {
    const m = text.match(/\n*\{\s*"box_2d"\s*:\s*\[\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\]\s*\}\s*$/);
    if (!m) return { text, box: null };
    const nums = m.slice(1, 5).map(Number) as Box2d;
    // Model thỉnh thoảng trả toạ độ lệch khỏi [0,1000] (VD làm tròn quá đà) —
    // kẹp lại cho chắc, không để khung vẽ tràn ra ngoài ảnh.
    const box: Box2d = nums.map((n) => Math.min(1000, Math.max(0, n))) as Box2d;
    return { text: text.slice(0, m.index).trimEnd(), box };
  }

  /** Khung theo TỪNG LƯỢT trả lời (key = index trong `history`) — không phải
   * chỉ giữ "khung gần nhất" nữa: hiện trực tiếp ngay trong bong bóng chat
   * của đúng câu trả lời đó, nên cuộn lại xem câu trả lời cũ vẫn thấy đúng
   * khung của câu đó, không bị đè bởi câu hỏi sau. */
  let turnBoxes = $state<Record<number, Box2d>>({});
  /** Khung của câu trả lời GẦN NHẤT — dùng riêng cho ảnh phóng to (modal),
   * nơi chỉ có 1 chỗ hiển thị chung cho cả cuộc hội thoại. */
  let latestBox = $derived<Box2d | null>(turnBoxes[history.length - 1] ?? null);

  /** Vùng đang chờ hỏi thêm (bấm "Hỏi thêm" trên khung trong ảnh phóng to) —
   * dùng 1 LẦN cho câu hỏi tiếp theo rồi tự xoá, không "dính" mãi như khoảng
   * thời gian video (giữ tối giản: 1 nút, 1 lần dùng, không cần thêm state
   * "đang bật/tắt" phức tạp). */
  let pendingRegion = $state<Box2d | null>(null);

  function handleAskRegion(box: Box2d) {
    pendingRegion = box;
    showImagePreview = false;
    followupInputEl?.focus();
  }

  /** `region`: chỉ có khi lượt hỏi này đến từ nút "Hỏi thêm" trên khung
   * khoanh vùng — Rust sẽ cắt tạm đúng vùng đó để gửi CHO ĐÚNG LƯỢT NÀY
   * (xem region param của ask_ai_gemini).
   * `search`: bật "Tra cứu web thật" (Google Search grounding) CHO ĐÚNG
   * LƯỢT NÀY — chỉ Gemini hỗ trợ, tính phí theo lượt nên không mặc định bật. */
  async function runTurn(region?: Box2d | null, search?: boolean) {
    busy = true;
    streamChunks = [];
    error = "";
    await scrollToBottom();

    const startedAt = Date.now();
    elapsedSec = 0;
    const timer = setInterval(() => {
      elapsedSec = Math.round((Date.now() - startedAt) / 1000);
    }, 1000);

    try {
      const settings = loadSettings();

      // Trễ hiển thị ~50 ký tự cuối so với luồng stream thật — KHÔNG đưa
      // thẳng từng mẩu vào hiệu ứng "gõ chữ" ngay khi nhận được. Lý do: dòng
      // JSON `{"box_2d":[...]}` (nếu có) luôn nằm Ở CUỐI câu trả lời, và nếu
      // hiện thẳng theo stream thì người dùng sẽ thấy nó NHÁY LÊN vài trăm ms
      // trước khi bị cắt bỏ ở bước làm sạch cuối cùng (extractBoxFromAnswer)
      // — trông như lỗi hiển thị. Giữ lại 50 ký tự cuối (đủ dư so với độ dài
      // dòng JSON thực tế, ~35 ký tự) chưa hiện ngay, đợi đủ dữ liệu mới hiện
      // tiếp; phần đuôi còn lại (rất ngắn) chỉ đơn giản xuất hiện cùng lúc
      // khi bong bóng chuyển sang bản render markdown đã làm sạch — mất hiệu
      // ứng fade-in ở đúng vài chục ký tự cuối, không đáng kể.
      let rawSoFar = "";
      let shownLength = 0;
      const REVEAL_LAG_CHARS = 50;
      const answer = await askAIStream(
        history,
        settings,
        (piece) => {
          rawSoFar += piece;
          const safeUpTo = Math.max(0, rawSoFar.length - REVEAL_LAG_CHARS);
          if (safeUpTo > shownLength) {
            pushStreamPiece(rawSoFar.slice(shownLength, safeUpTo));
            shownLength = safeUpTo;
          }
          scrollToBottom();
        },
        (s) => (statusLine = s),
        region,
        search,
      );
      const { text: cleanAnswer, box } = isVideoSession ? { text: answer, box: null } : extractBoxFromAnswer(answer);
      const newTurnIndex = history.length; // đúng vị trí lượt assistant sắp thêm vào bên dưới
      if (box) turnBoxes = { ...turnBoxes, [newTurnIndex]: box };
      history = [...history, { role: "assistant", content: cleanAnswer }];
      saveHistoryTurn(settings);
    } catch (e) {
      // Luôn hiện lỗi + không bao giờ để `busy` treo mãi (bug đã gặp trước đây:
      // UI đứng im ở trạng thái đang chờ mà không báo gì).
      error = String(e).replace(/^Error:\s*/, "");
    } finally {
      clearInterval(timer);
      statusLine = "";
      streamChunks = [];
      busy = false;
      await scrollToBottom();
    }
  }

  /** Hậu tố "(00:05)" / "(00:05–00:12)" gắn vào displayLabel khi có chỉ định
   * thời điểm — để bong bóng chat TỰ ghi lại đã hỏi trong phạm vi nào, không
   * cần người dùng nhớ lại. Chỉ áp dụng phiên video. */
  const timeBadgeSuffix = $derived(isVideoSession && rangeTouched ? ` (${timeRangeLabel})` : "");

  /** Bật "Tra cứu web thật" (Google Search grounding) — CHỈ Gemini hỗ trợ,
   * tính phí theo lượt Google tự quyết định search, nên KHÔNG mặc định bật
   * và tự tắt lại sau mỗi câu hỏi (dùng 1 lần, giống `pendingRegion`), tránh
   * đốt quota vô tình khi hỏi tiếp những câu không cần tra cứu gì cả. */
  let searchEnabled = $state(false);

  async function handleAsk() {
    const typed = question.trim();
    const q = typed || (isVideoSession ? PROMPT_VIDEO_EXPLAIN : PROMPT_EXPLAIN);
    const search = searchEnabled;
    searchEnabled = false;
    const suffix = `${timeBadgeSuffix}${search ? " 🌐" : ""}`;
    const displayLabel = suffix ? `${typed || "Giải thích nội dung"}${suffix}` : undefined;
    history = [{ role: "user", content: q + timeContextSuffix, displayLabel }];
    phase = "chat";
    await scrollToBottom();
    runTurn(undefined, search);
  }

  /** Gửi thẳng 1 chip gợi ý — bong bóng chat hiện `chip.chatLabel` ngắn gọn,
   * còn `chip.prompt` (đầy đủ, chi tiết) mới là thứ thực sự gửi cho AI. Có
   * chỉ định thời điểm thì nối thêm `timeContextSuffix` vào nội dung thật
   * (AI đọc) và `timeBadgeSuffix` vào nhãn hiển thị (người dùng thấy). */
  async function askWithPrompt(chip: QuickPrompt) {
    history = [
      { role: "user", content: chip.prompt + timeContextSuffix, displayLabel: chip.chatLabel + timeBadgeSuffix },
    ];
    phase = "chat";
    await scrollToBottom();
    runTurn();
  }

  function handleSend() {
    const q = followupText.trim();
    if (!q || busy) return;
    followupText = "";
    const region = pendingRegion;
    pendingRegion = null; // dùng 1 lần cho câu hỏi này rồi tự xoá
    const search = searchEnabled;
    searchEnabled = false; // tương tự — dùng 1 lần cho câu hỏi này rồi tự tắt
    const suffix = `${timeBadgeSuffix}${search ? " 🌐" : ""}`;
    const displayLabel = suffix ? `${q}${suffix}` : undefined;
    history = [...history, { role: "user", content: q + timeContextSuffix, displayLabel }];
    scrollToBottom();
    runTurn(region, search);
  }

  async function handleCopy() {
    // Nút Copy ở header: chép câu trả lời AI GẦN NHẤT — tiện khi chỉ có 1 lượt
    // hỏi hoặc chỉ quan tâm kết quả mới nhất. Muốn chép 1 câu trả lời CŨ hơn
    // (khi đã hỏi tiếp nhiều lần) thì dùng nút copy nhỏ trên từng bong bóng.
    const lastAi = [...history].reverse().find((t) => t.role === "assistant");
    if (!lastAi) return;
    try {
      // Chép văn bản SẠCH (không dính cú pháp Markdown thô) — người dùng dán
      // thẳng vào email/Word/Zalo, xem markdownToPlainText() để hiểu vì sao.
      await writeText(markdownToPlainText(lastAi.content));
      copyFlash = true;
      setTimeout(() => (copyFlash = false), 1600);
    } catch (e) {
      error = String(e);
    }
  }

  // Copy TỪNG câu trả lời riêng lẻ, không chỉ câu cuối — theo dõi index vừa
  // copy để hiện tick ✓ đúng bong bóng đó (không phải hiện chung cho cả UI).
  let copiedTurnIndex = $state<number | null>(null);
  async function handleCopyTurn(i: number, content: string) {
    try {
      await writeText(markdownToPlainText(content));
      copiedTurnIndex = i;
      setTimeout(() => {
        if (copiedTurnIndex === i) copiedTurnIndex = null;
      }, 1600);
    } catch (e) {
      error = String(e);
    }
  }

  function handleClose() {
    getCurrentWindow().close();
  }

  function onPreviewKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") closeImagePreview();
  }
</script>

<svelte:window onkeydown={showImagePreview ? onPreviewKeydown : undefined} />

<div class="app-bg h-screen flex flex-col text-text overflow-hidden">
  {#snippet chainThumbnails()}
    <!-- Dải thumbnail cho chuỗi nhiều ảnh/video ("+ Chụp thêm bước") — chỉ
    hiện khi có từ 2 media trở lên, đặt trong modal (đủ chỗ hơn hẳn header
    32px chật hẹp lúc đang chat). Bấm vào 1 cái ghim preview vào đúng cái đó. -->
    <div class="flex gap-1.5 overflow-x-auto max-w-full pb-1">
      {#each mediaChain as item, i (i)}
        <button
          type="button"
          onclick={() => (previewIndex = i)}
          class="shrink-0 w-12 h-12 rounded-lg overflow-hidden border-2 transition-colors {(previewIndex ?? mediaChain.length - 1) ===
          i
            ? 'border-accent'
            : 'border-border hover:border-accent/50'}"
          title={`Bước ${i + 1}`}
        >
          {#if isVideoSession}
            <video src={`data:video/mp4;base64,${item}`} muted class="w-full h-full object-cover"></video>
          {:else}
            <img src={`data:image/png;base64,${item}`} alt={`Bước ${i + 1}`} class="w-full h-full object-cover" />
          {/if}
        </button>
      {/each}
    </div>
  {/snippet}

  {#snippet timeRangeSlider(onDone?: () => void)}
    <!-- Thanh chọn thời điểm/khoảng — 2 thanh <input type=range> chồng lên
    nhau (kỹ thuật dual-range kinh điển: mỗi input tự lo 1 tay cầm, CSS cho
    track trong suốt để chỉ thấy phần "đã tô" ở giữa). Kéo trùng 2 tay cầm
    thì thành "1 thời điểm" thay vì "1 khoảng" (xem isTimePoint). Dùng chung
    (snippet) cho cả 2 chỗ: lúc mới mở (`phase === "ask"`) VÀ lúc đang chat
    (bấm nút "Chọn khoảng thời gian" để mở lại) — trước đây chỉ có ở chỗ đầu,
    hỏi tiếp về 1 mốc khác trong cùng video phải đóng cửa sổ snip lại từ đầu. -->
    <div class="shrink-0 px-3 pt-2.5">
      <div class="flex items-center justify-between mb-1">
        <span class="text-[10.5px] text-text-muted flex items-center gap-1">
          <Icon name="target" size={11} />
          {rangeTouched ? `Đang hỏi về ${isTimePoint ? "thời điểm" : "khoảng"} ${timeRangeLabel}` : "Kéo để hỏi về 1 thời điểm/khoảng cụ thể"}
        </span>
        <div class="flex items-center gap-2">
          {#if rangeTouched}
            <button
              type="button"
              onclick={clearTimeRange}
              class="text-[10.5px] text-text-muted hover:text-text transition-colors flex items-center gap-0.5"
            >
              <Icon name="x" size={10} /> Bỏ chọn
            </button>
          {/if}
          {#if onDone}
            <!-- Chỉ có ở bản dùng TRONG CHAT (truyền onDone) — thu gọn thanh
            lại sau khi đã chọn xong, tránh chiếm chỗ mãi trong lúc gõ hỏi tiếp. -->
            <button type="button" onclick={onDone} class="text-[10.5px] text-accent font-medium hover:brightness-110 transition-all">
              Xong
            </button>
          {/if}
        </div>
      </div>
      <div class="relative h-4 flex items-center">
        <div class="absolute inset-x-0 h-1 rounded-full bg-bg-elevated"></div>
        <div
          class="absolute h-1 rounded-full"
          style="left:{(rangeStart / videoDuration) * 100}%; right:{100 - (rangeEnd / videoDuration) * 100}%; background: linear-gradient(90deg, var(--color-accent), var(--color-accent-2));"
        ></div>
        <input
          type="range"
          min="0"
          max={videoDuration}
          step="0.1"
          value={rangeStart}
          class="range-thumb"
          oninput={(e) => {
            const v = Math.min(Number(e.currentTarget.value), rangeEnd);
            rangeStart = v;
            rangeTouched = true;
            seekPreview(v);
          }}
        />
        <input
          type="range"
          min="0"
          max={videoDuration}
          step="0.1"
          value={rangeEnd}
          class="range-thumb"
          oninput={(e) => {
            const v = Math.max(Number(e.currentTarget.value), rangeStart);
            rangeEnd = v;
            rangeTouched = true;
            seekPreview(v);
          }}
        />
      </div>
    </div>
  {/snippet}

  {#if phase === "ask"}
    <!-- ── Giai đoạn 1: xem ảnh/video + đặt câu hỏi ── -->
    <div class="flex-1 min-h-0 p-3 pb-0 flex items-center justify-center">
      {#if mediaB64}
        {#if isVideoSession}
          <video
            bind:this={videoEl}
            onloadedmetadata={onVideoLoadedMetadata}
            src={`data:video/mp4;base64,${mediaB64}`}
            controls
            autoplay
            muted
            loop
            class="max-w-full max-h-full rounded-xl border border-border shadow-lg"
            transition:fade={{ duration: 180 }}
          ></video>
        {:else}
          <img
            src={`data:image/png;base64,${mediaB64}`}
            alt="Vùng đã chụp"
            class="max-w-full max-h-full object-contain rounded-xl border border-border shadow-lg"
            transition:fade={{ duration: 180 }}
          />
        {/if}
      {:else if !error}
        <!-- Cửa sổ mở ngay khi vừa chọn xong vùng/quay xong, ảnh/video còn
        đang xử lý ở backend — hiện loading thay vì để khoảng trống im lặng. -->
        <div class="flex flex-col items-center gap-2 text-text-muted" transition:fade={{ duration: 140 }}>
          <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
          <span class="text-[11px]">{isVideoSession ? "Đang xử lý video…" : "Đang xử lý ảnh…"}</span>
        </div>
      {/if}
    </div>

    {#if isVideoSession && videoDuration > 0}
      {@render timeRangeSlider()}
    {/if}

    <div class="shrink-0 px-3 pt-3 flex flex-wrap gap-1.5">
      {#each quickPrompts as chip (chip.id)}
        <button class="chip disabled:opacity-40" disabled={!mediaB64} onclick={() => askWithPrompt(chip)}>
          <Icon name={chip.icon} size={13} />
          {chip.label}
        </button>
      {/each}
    </div>

    <div class="shrink-0 p-3 flex gap-2">
      <input
        type="text"
        bind:value={question}
        disabled={!mediaB64}
        placeholder={mediaB64
          ? `Hỏi bất kỳ điều gì về ${isVideoSession ? "video" : "vùng"} đã ${isVideoSession ? "quay" : "chụp"}…`
          : isVideoSession
            ? "Đang xử lý video…"
            : "Đang xử lý ảnh…"}
        class="field selectable flex-1 disabled:opacity-50"
        onkeydown={(e) => e.key === "Enter" && handleAsk()}
      />
      <button
        type="button"
        onclick={() => (searchEnabled = !searchEnabled)}
        disabled={!mediaB64}
        title="Tra cứu web thật khi trả lời (Google Search) — chỉ áp dụng cho câu hỏi này"
        aria-pressed={searchEnabled}
        class="rounded-lg px-2.5 flex items-center justify-center transition-colors disabled:opacity-40 {searchEnabled
          ? 'btn-accent'
          : 'btn-ghost'}"
      >
        <Icon name="globe" size={15} />
      </button>
      <button
        onclick={handleAsk}
        disabled={!mediaB64}
        class="btn-accent px-4 rounded-lg text-[13px] flex items-center gap-1.5 disabled:opacity-40"
      >
        <Icon name="send" size={15} strokeWidth={2.2} />
      </button>
    </div>
  {:else}
    <!-- ── Giai đoạn 2: hội thoại ── -->
    <div class="glass shrink-0 h-12 flex items-center px-3 gap-2.5">
      {#if mediaB64}
        <button
          type="button"
          onclick={openMediaPreview}
          class="shrink-0 w-8 h-8 rounded-lg overflow-hidden border border-border hover:border-accent/60 transition-colors relative group"
          title={isVideoSession ? "Xem lại video đã quay" : "Xem lại ảnh đã chụp"}
        >
          {#if isVideoSession}
            <video src={`data:video/mp4;base64,${mediaB64}`} muted class="w-full h-full object-cover"></video>
          {:else}
            <img src={`data:image/png;base64,${mediaB64}`} alt="Vùng đã chụp" class="w-full h-full object-cover" />
          {/if}
          <span
            class="absolute inset-0 bg-black/0 group-hover:bg-black/35 flex items-center justify-center transition-colors"
          >
            <Icon name="eye" size={13} class="text-white opacity-0 group-hover:opacity-100 transition-opacity" />
          </span>
          {#if mediaChain.length > 1}
            <!-- Có nhiều ảnh/video (chuỗi "+ Chụp thêm bước") — báo ngay ở
            đây, không cần mở ảnh phóng to mới biết. -->
            <span
              class="absolute -bottom-0.5 -right-0.5 min-w-[15px] h-[15px] px-0.5 rounded-full bg-accent text-accent-text text-[9px] font-bold flex items-center justify-center leading-none"
            >
              {mediaChain.length}
            </span>
          {/if}
        </button>
      {:else}
        <div
          class="w-6 h-6 rounded-lg flex items-center justify-center text-accent-text shrink-0 {busy ? 'pulse-ring' : ''}"
          style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
        >
          <Icon name="sparkles" size={13} strokeWidth={2.3} />
        </div>
      {/if}
      <div class="flex-1 min-w-0">
        <div class="text-[12.5px] font-semibold leading-tight">Kết quả AI</div>
      </div>
      <button
        onclick={handleCopy}
        disabled={busy}
        class="btn-ghost px-2.5 py-1.5 rounded-lg text-[11.5px] flex items-center gap-1.5 disabled:opacity-40"
        title="Sao chép câu trả lời"
      >
        <Icon name={copyFlash ? "check" : "copy"} size={13} />
        {copyFlash ? "Đã chép" : "Chép"}
      </button>
      <button onclick={handleClose} class="btn-ghost p-1.5 rounded-lg" title="Đóng">
        <Icon name="x" size={14} />
      </button>
    </div>

    {#if busy}
      <div class="shrink-0 h-0.5 shimmer" transition:fade={{ duration: 120 }}></div>
    {/if}

    <ScrollArea
      bind:viewport={transcriptEl}
      class="selectable flex-1 min-h-0"
      contentClass="px-3 py-3.5 flex flex-col gap-3.5"
    >
      {#each history as turn, i (i)}
        {#if turn.role === "user"}
          <div class="flex justify-end msg-in">
            <div
              class="max-w-[86%] rounded-2xl rounded-br-md px-3.5 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap text-accent-text font-medium"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              {turn.displayLabel ?? turn.content}
            </div>
          </div>
        {:else}
          <div class="flex items-start gap-2.5 msg-in group">
            <div
              class="shrink-0 w-6 h-6 mt-0.5 rounded-lg flex items-center justify-center text-accent-text"
              style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
            >
              <Icon name="sparkles" size={12} strokeWidth={2.3} />
            </div>
            <div class="flex flex-col gap-1.5 max-w-[88%]">
              {#if turnBoxes[i] && mediaB64}
                <!-- Ảnh nhỏ kèm khung AI chỉ tới — hiện NGAY tại đây thay vì
                bắt người dùng tự bấm mở ảnh phóng to mới thấy, dễ quan sát
                hơn hẳn. Bấm vào vẫn mở được ảnh phóng to như bình thường. -->
                <button
                  type="button"
                  onclick={openMediaPreview}
                  class="self-start rounded-xl overflow-hidden border border-border hover:border-accent/60 transition-colors"
                  title="Xem ảnh phóng to"
                >
                  <BoxedImage
                    src={`data:image/png;base64,${mediaB64}`}
                    box={turnBoxes[i]}
                    alt="Vùng AI chỉ tới"
                    class="max-h-40 object-contain"
                  />
                </button>
              {/if}
              <div class="relative">
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- 1 listener DUY NHẤT bọc ngoài, tự dò đúng nút mốc giờ vừa
                bấm (xem handleTimestampClick) — không gắn onclick trực tiếp
                vào chuỗi HTML vì DOMPurify đã xoá sạch onclick lúc sanitize. -->
                <div
                  class="markdown-body card rounded-2xl rounded-tl-md px-3.5 py-2.5 pr-8 text-[12.5px]"
                  onclick={handleAnswerClick}
                >
                  {@html renderMarkdown(isVideoSession ? linkifyTimestamps(turn.content) : turn.content)}
                </div>
              <button
                onclick={() => handleCopyTurn(i, turn.content)}
                class="absolute top-1.5 right-1.5 p-1.5 rounded-md text-text-muted hover:text-accent hover:bg-white/8 opacity-0 group-hover:opacity-100 transition-opacity"
                title="Sao chép câu trả lời này"
              >
                <Icon name={copiedTurnIndex === i ? "check" : "copy"} size={12} />
              </button>
              </div>
            </div>
          </div>
        {/if}
      {/each}

      {#if busy}
        <div class="flex items-start gap-2.5">
          <div
            class="shrink-0 w-6 h-6 mt-0.5 rounded-lg flex items-center justify-center text-accent-text pulse-ring"
            style="background: linear-gradient(135deg, var(--color-accent), var(--color-accent-2));"
          >
            <Icon name="sparkles" size={12} strokeWidth={2.3} />
          </div>
          <div class="card max-w-[88%] rounded-2xl rounded-tl-md px-3.5 py-2.5 text-[12.5px] leading-relaxed">
            {#if streamChunks.length === 0}
              <span class="thinking-dots inline-flex items-center h-4"><span></span><span></span><span></span></span>
            {:else}
              <span class="whitespace-pre-wrap"
                >{#each streamChunks as chunk (chunk.id)}<span in:fade={{ duration: 170 }}>{chunk.text}</span
                  >{/each}<span class="typing-cursor"></span></span
              >
            {/if}
          </div>
        </div>
      {/if}

      {#if error}
        <div
          class="card border-[color:var(--color-danger)]/40 px-3.5 py-2.5 text-[12px] text-[color:var(--color-danger)] flex items-start gap-2 msg-in"
        >
          <Icon name="alert" size={14} class="shrink-0 mt-0.5" />
          <span class="selectable leading-relaxed">{error}</span>
        </div>
      {/if}
    </ScrollArea>

    <div class="shrink-0 px-3 pb-3 pt-1 flex flex-col gap-1.5">
      {#if isVideoSession}
        <!-- Bấm vào đây MỞ LUÔN ảnh phóng to kèm thanh kéo bên trong (xem
        modal ở cuối file) — vừa xem lại video vừa chọn khoảng dễ hơn hẳn so
        với kéo "mù" (không thấy hình) ngay trong khung hội thoại chật hẹp. -->
        {#if rangeTouched}
          <!-- Khoảng chọn vẫn còn hiệu lực cho câu hỏi tiếp theo (sticky) —
          nhắc lại ở đây để người dùng không quên đang giới hạn phạm vi hỏi. -->
          <div class="flex items-center gap-1.5 text-[10.5px] text-accent px-0.5">
            <button
              type="button"
              onclick={openRangeSliderModal}
              class="flex items-center gap-1.5 hover:brightness-110 transition-all"
            >
              <Icon name="target" size={11} />
              Đang hỏi về {isTimePoint ? "thời điểm" : "khoảng"} {timeRangeLabel}
            </button>
            <button type="button" onclick={clearTimeRange} class="text-text-muted hover:text-text transition-colors ml-0.5">
              <Icon name="x" size={10} />
            </button>
          </div>
        {:else}
          <button type="button" onclick={openRangeSliderModal} class="chip self-start">
            <Icon name="target" size={12} />
            Chọn khoảng thời gian
          </button>
        {/if}
      {/if}
      {#if pendingRegion}
        <!-- Cùng kiểu chip với "đang hỏi về khoảng ..." của video — nhất
        quán, không phải học thêm 1 kiểu UI mới. -->
        <div class="flex items-center gap-1.5 text-[10.5px] text-accent px-0.5">
          <Icon name="target" size={11} />
          Đang hỏi về vùng đã chọn
          <button type="button" onclick={() => (pendingRegion = null)} class="text-text-muted hover:text-text transition-colors ml-0.5">
            <Icon name="x" size={10} />
          </button>
        </div>
      {/if}
      {#if busy && statusLine}
        <div class="text-[10.5px] text-text-muted flex items-center gap-1.5 px-0.5" transition:fade={{ duration: 140 }}>
          <span class="truncate">{statusLine}</span>
          <span class="text-accent font-semibold shrink-0">{elapsedSec}s</span>
        </div>
      {/if}
      <div class="flex gap-2">
        <input
          bind:this={followupInputEl}
          type="text"
          bind:value={followupText}
          disabled={busy}
          placeholder={busy ? "AI đang trả lời…" : pendingRegion ? "Hỏi về vùng đã chọn…" : "Hỏi tiếp…"}
          class="field selectable flex-1 disabled:opacity-50"
          onkeydown={(e) => e.key === "Enter" && handleSend()}
        />
        <button
          type="button"
          onclick={triggerAppendCapture}
          disabled={busy || appendCaptureBusy}
          title={`${isVideoSession ? "Quay" : "Chụp"} thêm bước tiếp theo, thêm vào cùng cuộc hội thoại này`}
          class="btn-ghost rounded-lg px-2.5 flex items-center justify-center transition-colors disabled:opacity-40"
        >
          {#if appendCaptureBusy}
            <Icon name="loader" size={15} class="animate-spin" />
          {:else}
            <Icon name="plus" size={15} />
          {/if}
        </button>
        <button
          type="button"
          onclick={() => (searchEnabled = !searchEnabled)}
          disabled={busy}
          title="Tra cứu web thật khi trả lời (Google Search) — chỉ áp dụng cho câu hỏi này"
          aria-pressed={searchEnabled}
          class="rounded-lg px-2.5 flex items-center justify-center transition-colors disabled:opacity-40 {searchEnabled
            ? 'btn-accent'
            : 'btn-ghost'}"
        >
          <Icon name="globe" size={15} />
        </button>
        <button onclick={handleSend} disabled={busy} class="btn-accent px-4 rounded-lg">
          <Icon name="send" size={15} strokeWidth={2.2} />
        </button>
      </div>
    </div>
  {/if}

  {#if showImagePreview && mediaB64}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="fixed inset-0 z-40 bg-black/80 backdrop-blur-sm flex items-center justify-center p-6"
      role="presentation"
      onclick={closeImagePreview}
      transition:fade={{ duration: 140 }}
    >
      {#if isVideoSession}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_media_has_caption -->
        <!-- Video tự quay bằng app này KHÔNG có track âm thanh (đã tắt hẳn ở
        record.rs) nên không có gì để phụ đề — cảnh báo a11y này không áp
        dụng được. Bọc chung video + thanh kéo trong 1 khối chặn nổi bọt
        click — thanh kéo nằm NGOÀI thẻ <video> nên cần chặn riêng, không
        thì bấm vào nó sẽ đóng mất modal (tính là bấm "ra ngoài"). -->
        <div onclick={(e) => e.stopPropagation()} class="flex flex-col items-center gap-3 max-w-[85vw]">
          <video
            bind:this={modalVideoEl}
            onloadedmetadata={onModalVideoReady}
            src={`data:video/mp4;base64,${previewMediaB64}`}
            controls
            autoplay
            class="max-w-full max-h-[70vh] rounded-xl border border-border shadow-2xl"
          ></video>
          {#if mediaChain.length > 1}
            {@render chainThumbnails()}
          {/if}
          {#if showRangeSliderInChat}
            <div class="card p-2.5 w-full">
              {@render timeRangeSlider(() => {
                showRangeSliderInChat = false;
                showImagePreview = false;
              })}
            </div>
          {/if}
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- Chỉ chặn nổi bọt click (đóng modal khi bấm ra ngoài ảnh) — không
        phải phần tử tương tác thật, nên không cần bàn phím điều khiển. -->
        <!-- min-w-0/min-h-0: ĐÈ mặc định `min-width:auto` của flex item —
        thiếu 2 dòng này, div bọc có thể to hơn cả khung modal chứa nó theo
        kích thước ẢNH THẬT (ảnh chụp màn hình có thể rất lớn), làm ảnh bị
        TRÀN RA NGOÀI/CẮT MẤT thay vì co lại vừa khung như class max-w-full
        max-h-full mong muốn. Bug thực tế đã gặp: thêm lớp div chặn click này
        làm đứt quãng chuỗi ràng buộc kích thước vốn nằm thẳng trên <img>
        trước đây. -->
        <div class="max-w-full max-h-full min-w-0 min-h-0" onclick={(e) => e.stopPropagation()}>
          <!-- max-w/max-h theo vw/vh (KHÔNG theo %) — luôn tính theo kích
          thước cửa sổ thật, không phụ thuộc chuỗi div cha có "auto" hay
          không, tránh đúng bug đã gặp: ảnh hiển thị to hơn khung, bị cắt mất
          không xem được toàn bộ. Trừ hao 1 khoảng (85/80 thay vì 100) để
          không dính sát mép modal (p-6 + nút đóng ở góc). -->
          <div class="flex flex-col items-center gap-3">
            <BoxedImage
              src={`data:image/png;base64,${previewMediaB64}`}
              box={previewIndex === null || previewIndex === mediaChain.length - 1 ? latestBox : null}
              alt="Vùng đã chụp (phóng to)"
              class="max-w-[85vw] max-h-[80vh] object-contain rounded-xl border border-border shadow-2xl"
              interactive
              onAskRegion={handleAskRegion}
            />
            {#if mediaChain.length > 1}
              {@render chainThumbnails()}
            {/if}
          </div>
        </div>
      {/if}
      <button onclick={closeImagePreview} class="absolute top-4 right-4 btn-ghost p-2 rounded-lg" title="Đóng (Esc)">
        <Icon name="x" size={18} />
      </button>
    </div>
  {/if}
</div>
