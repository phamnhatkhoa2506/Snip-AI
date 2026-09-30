// Đọc câu trả lời AI thành giọng nói — giọng Gemini TTS (tự nhiên, qua
// backend, xem tts.rs) là chính; lỗi (hết quota, mạng...) thì tự chuyển sang
// giọng có sẵn của Windows (speechSynthesis của WebView2) thay vì im lặng.
//
// Chỉ 1 đoạn được đọc tại 1 thời điểm trong mỗi cửa sổ: bắt đầu đọc đoạn mới
// thì dừng đoạn cũ (`playToken` loại bỏ kết quả TTS về muộn của lượt đã bị
// huỷ, tránh 2 giọng chồng lên nhau).

import { invoke } from "@tauri-apps/api/core";
import { markdownToPlainText } from "$lib/markdown";
import { currentTtsModel, currentVoice, loadSettings } from "$lib/settings";

/** Đọc quá dài vừa tốn quota vừa không ai nghe hết — cắt ở ranh giới câu. */
const MAX_SPEECH_CHARS = 2500;

/** Bỏ những thứ đọc thành tiếng vô nghĩa (khối code, sơ đồ/biểu đồ JSON,
 * công thức LaTeX, bảng) — người nghe vẫn xem được chúng trên màn hình. */
export function speechTextFromMarkdown(md: string): string {
  let s = md
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/\$\$[\s\S]*?\$\$/g, " ")
    .replace(/\\\[[\s\S]*?\\\]/g, " ")
    .replace(/\\\([\s\S]*?\\\)/g, " ")
    .replace(/^\s*\|.*\|\s*$/gm, " ");
  s = markdownToPlainText(s)
    .replace(/^•\s*/gm, "")
    .replace(/\[(\d{1,2}:\d{2}(?::\d{2})?)\]/g, "$1")
    .replace(/[ \t]+/g, " ")
    .replace(/\n{2,}/g, "\n")
    .trim();
  if (s.length > MAX_SPEECH_CHARS) {
    const cut = s.slice(0, MAX_SPEECH_CHARS);
    const lastStop = Math.max(cut.lastIndexOf(". "), cut.lastIndexOf("\n"), cut.lastIndexOf("! "), cut.lastIndexOf("? "));
    s = `${lastStop > MAX_SPEECH_CHARS * 0.6 ? cut.slice(0, lastStop + 1) : cut} …`;
  }
  return s;
}

let currentAudio: HTMLAudioElement | null = null;
let playToken = 0;

export function stopSpeaking() {
  playToken++;
  const audio = currentAudio;
  currentAudio = null;
  audio?.pause();
  if ("speechSynthesis" in window) window.speechSynthesis.cancel();
}

/** Đọc 1 câu trả lời Markdown. Resolve khi đọc xong HOẶC bị dừng/lỗi —
 * không bao giờ reject (lỗi đã tự chuyển sang giọng Windows). */
export async function speakMarkdown(md: string): Promise<void> {
  stopSpeaking();
  const token = ++playToken;
  const text = speechTextFromMarkdown(md);
  if (!text) return;
  try {
    const s = loadSettings();
    const wavB64 = await invoke<string>("speak_text", { text, model: currentTtsModel(s), voice: currentVoice(s) });
    if (token !== playToken) return;
    await playAudio(`data:audio/wav;base64,${wavB64}`);
  } catch (e) {
    if (token !== playToken) return;
    console.warn("[snip-ai] Giọng Gemini lỗi, chuyển sang giọng Windows:", e);
    await speakWithSystemVoice(text);
  }
}

function playAudio(src: string): Promise<void> {
  return new Promise((resolve) => {
    const audio = new Audio(src);
    currentAudio = audio;
    const done = () => {
      if (currentAudio === audio) currentAudio = null;
      resolve();
    };
    audio.onended = done;
    audio.onerror = done;
    audio.onpause = done;
    audio.play().catch(done);
  });
}

function speakWithSystemVoice(text: string): Promise<void> {
  return new Promise((resolve) => {
    if (!("speechSynthesis" in window)) return resolve();
    const utterance = new SpeechSynthesisUtterance(text);
    const vi = window.speechSynthesis.getVoices().find((v) => v.lang.toLowerCase().startsWith("vi"));
    if (vi) utterance.voice = vi;
    utterance.lang = vi?.lang ?? "vi-VN";
    utterance.onend = () => resolve();
    utterance.onerror = () => resolve();
    window.speechSynthesis.speak(utterance);
  });
}
