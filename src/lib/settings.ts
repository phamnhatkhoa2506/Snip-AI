// Cấu hình KHÔNG nhạy cảm (provider đang chọn, tên model, reasoning effort)
// lưu bằng localStorage — dùng chung giữa các cửa sổ vì cùng origin.
//
// ⚠️ API KEY KHÔNG nằm ở đây. Key được lưu trong Windows Credential Manager
// (OS keychain) qua các lệnh Rust ở secrets.rs, và KHÔNG BAO GIỜ được trả về
// frontend. Frontend chỉ có thể: ghi key mới, hỏi "đã có key chưa" (kèm 4 ký
// tự cuối để nhận diện), và xoá key.

import { invoke } from "@tauri-apps/api/core";

export type Provider = "nvidia" | "openai" | "anthropic" | "gemini";

export interface Settings {
  provider: Provider;
  nvidiaModel: string;
  /** Chỉ 1 số model NVIDIA hỗ trợ "reasoning" (VD DeepSeek-R1, QwQ, Kimi-K2).
   * Đa số model vision thường (VD Llama vision) KHÔNG có khái niệm này — nên
   * mặc định TẮT, người dùng tự bật khi biết chắc model của mình hỗ trợ. */
  nvidiaReasoningEnabled: boolean;
  nvidiaReasoningEffort: string;
  openaiModel: string;
  anthropicModel: string;
  geminiModel: string;
}

const STORAGE_KEY = "snip-ai:settings";

export const DEFAULT_SETTINGS: Settings = {
  provider: "nvidia",
  nvidiaModel: "meta/llama-3.2-11b-vision-instruct",
  nvidiaReasoningEnabled: false,
  nvidiaReasoningEffort: "none",
  openaiModel: "gpt-4o",
  anthropicModel: "claude-sonnet-5",
  geminiModel: "gemini-2.0-flash",
};

export interface ProviderMeta {
  id: Provider;
  label: string;
  /** Nơi lấy API key, hiện trong UI để người dùng biết đi đâu lấy */
  keyUrl: string;
  keyPlaceholder: string;
  /** Trường trong Settings chứa tên model của provider này */
  modelField: keyof Settings;
  modelHint: string;
}

export const PROVIDERS: ProviderMeta[] = [
  {
    id: "nvidia",
    label: "NVIDIA NIM",
    keyUrl: "build.nvidia.com",
    keyPlaceholder: "nvapi-...",
    modelField: "nvidiaModel",
    modelHint: "Model phải hỗ trợ ảnh (vision). VD: meta/llama-3.2-11b-vision-instruct",
  },
  {
    id: "openai",
    label: "OpenAI",
    keyUrl: "platform.openai.com",
    keyPlaceholder: "sk-...",
    modelField: "openaiModel",
    modelHint: "Model phải hỗ trợ vision. VD: gpt-4o",
  },
  {
    id: "anthropic",
    label: "Anthropic",
    keyUrl: "console.anthropic.com",
    keyPlaceholder: "sk-ant-...",
    modelField: "anthropicModel",
    modelHint: "VD: claude-sonnet-5",
  },
  {
    id: "gemini",
    label: "Gemini",
    keyUrl: "aistudio.google.com",
    keyPlaceholder: "AIza...",
    modelField: "geminiModel",
    modelHint: "VD: gemini-2.0-flash",
  },
];

export function providerMeta(id: Provider): ProviderMeta {
  return PROVIDERS.find((p) => p.id === id) ?? PROVIDERS[0];
}

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(settings: Settings): void {
  const clean: Settings = {
    ...settings,
    nvidiaModel: settings.nvidiaModel.trim(),
    openaiModel: settings.openaiModel.trim(),
    anthropicModel: settings.anthropicModel.trim(),
    geminiModel: settings.geminiModel.trim(),
  };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(clean));
}

/** Tên model của provider đang chọn (đã trim — tránh lỗi từng gặp: model dán
 * dính khoảng trắng cuối khiến API trả HTTP 404 khó hiểu). */
export function currentModel(settings: Settings): string {
  return String(settings[providerMeta(settings.provider).modelField] ?? "").trim();
}

/** Ghi tên model cho provider đang chọn (type-safe, không ép kiểu Record). */
export function setCurrentModel(settings: Settings, value: string): void {
  switch (settings.provider) {
    case "nvidia":
      settings.nvidiaModel = value;
      break;
    case "openai":
      settings.openaiModel = value;
      break;
    case "anthropic":
      settings.anthropicModel = value;
      break;
    case "gemini":
      settings.geminiModel = value;
      break;
  }
}

// ── API key: chỉ đi qua Rust/OS keychain, không lưu ở JS ──────────────────

export interface KeyStatus {
  provider: Provider;
  hasKey: boolean;
  /** 4 ký tự cuối của key (VD "…a3f9") — đủ để nhận diện, không tái tạo được */
  hint: string;
}

export async function fetchKeyStatuses(): Promise<Record<Provider, KeyStatus>> {
  const list = await invoke<{ provider: string; has_key: boolean; hint: string }[]>("api_key_statuses");
  const map = {} as Record<Provider, KeyStatus>;
  for (const item of list) {
    map[item.provider as Provider] = {
      provider: item.provider as Provider,
      hasKey: item.has_key,
      hint: item.hint,
    };
  }
  return map;
}

export async function saveApiKey(provider: Provider, apiKey: string): Promise<void> {
  await invoke("save_api_key", { provider, apiKey });
}

export async function deleteApiKey(provider: Provider): Promise<void> {
  await invoke("delete_api_key", { provider });
}
