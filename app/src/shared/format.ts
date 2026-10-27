// Wording shared by the setup and settings pages.

import type { CatalogEntry, ModelFile, ModelKind, Platform, Tool } from "./types";

const MB = 1024 * 1024;

/** "547 MB" below 1 GB, "1.6 GB" above. */
export function formatBytes(bytes: number): string {
  const mb = bytes / MB;
  return mb < 1000 ? `${Math.max(1, Math.round(mb))} MB` : `${(mb / 1024).toFixed(1)} GB`;
}

export function formatMemory(mb: number): string {
  return `${(mb / 1024).toFixed(1)} GB`;
}

export const purposeLabel: Record<ModelKind, string> = {
  whisper: "文字起こし",
  llm: "文章の整え",
};

/** 標準モデル / 軽量モデル for catalog files, the file name for anything the user added. */
export function modelLabel(model: ModelFile, catalog: CatalogEntry[]): string {
  const entry = catalog.find((c) => c.file === model.name);
  const size = formatBytes(model.bytes);
  if (!entry) return `${model.name}（${size}）`;
  return `${entry.tier === "standard" ? "標準モデル" : "軽量モデル"}（${size}）`;
}

/** How to install a tool by hand, for machines without a download. */
export function installCommand(platform: Platform, tool: Tool): string | null {
  if (platform === "macOS") return tool === "whisper" ? "brew install whisper-cpp" : "brew install llama.cpp";
  if (platform === "windows" && tool === "llama") return "winget install llama.cpp";
  return null;
}

export const toolLabel: Record<Tool, string> = {
  whisper: "文字起こし（whisper.cpp）",
  llama: "文章の整え（llama.cpp）",
};

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
