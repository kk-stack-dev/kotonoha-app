// User-facing text for each session state. Key names come from the app (per OS, plain words),
// so no OS-specific glyphs or emoji appear in the UI.

import type { CopyReason, Failure, Outcome, Phase, Platform, UiContext } from "./types";

export type Tone = "neutral" | "recording" | "busy" | "success" | "info" | "warning";

export interface Message {
  tone: Tone;
  title: string;
  detail?: string;
}

export function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function phaseMessage(phase: Phase, ctx: UiContext): Message {
  switch (phase.phase) {
    case "idle":
      return { tone: "neutral", title: "待機中", detail: `${ctx.shortcutLabel} で録音` };
    case "starting":
      return { tone: "busy", title: "マイクを準備しています" };
    case "recording":
      return { tone: "recording", title: "録音中", detail: `${ctx.shortcutLabel} で停止` };
    case "transcribing":
      return {
        tone: "busy",
        title: "文字起こし中",
        detail: phase.remainingChunks > 1 ? `残り ${phase.remainingChunks} 件` : undefined,
      };
    case "rewriting":
      return { tone: "busy", title: "文章を整えています" };
    case "inserting":
      return { tone: "busy", title: "入力しています" };
    case "finished":
      return outcomeMessage(phase.outcome, ctx);
  }
}

function outcomeMessage(outcome: Outcome, ctx: UiContext): Message {
  switch (outcome.kind) {
    case "inserted":
      return { tone: "success", title: "入力しました" };
    case "copied":
      return { tone: "info", title: copyTitle(outcome.reason), detail: `${ctx.pasteLabel} で貼り付けできます` };
    case "failed":
      return { tone: "warning", ...failureText(outcome.failure, ctx.platform) };
  }
}

function copyTitle(reason: CopyReason): string {
  switch (reason) {
    case "autoInsertOff":
      return "クリップボードにコピーしました";
    case "permissionMissing":
      return "権限がないためクリップボードにコピーしました";
    case "secureField":
      return "パスワード入力欄のため、クリップボードにコピーしました";
    case "pasteFailed":
      return "自動入力に失敗したため、クリップボードにコピーしました";
  }
}

function micSettingsPath(platform: Platform): string {
  return platform === "windows"
    ? "設定 > プライバシーとセキュリティ > マイク"
    : "システム設定 > プライバシーとセキュリティ > マイク";
}

function failureText(failure: Failure, platform: Platform): { title: string; detail?: string } {
  switch (failure.code) {
    case "modelMissing":
      return { title: "音声認識モデルが見つかりません", detail: "設定画面からダウンロードしてください" };
    case "micPermission":
      return { title: "マイクへのアクセスが許可されていません", detail: micSettingsPath(platform) };
    case "micUnavailable":
      return { title: "マイクにアクセスできませんでした", detail: "入力デバイスの設定を確認してください" };
    case "micStartTimedOut":
      return { title: "マイクの起動がタイムアウトしました", detail: "ショートカットを押して再試行してください" };
    case "micLost":
      return { title: "録音中にマイクの接続が切れました" };
    case "noAudio":
      return {
        title: `${failure.detail} から音声が検出されませんでした`,
        detail: "マイクのミュート設定や入力機器をご確認ください（クラムシェルモード等では内蔵マイクが無効になる場合があります）",
      };
    case "tooShort":
      return { title: "録音時間が短すぎるため、認識できませんでした" };
    case "nothingRecognized":
      return { title: "音声を認識できませんでした" };
    case "transcription":
      return { title: "文字起こし処理に失敗しました", detail: "設定画面でツールの動作状況をご確認ください" };
  }
}
