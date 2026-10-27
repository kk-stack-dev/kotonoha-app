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
        detail: phase.remainingChunks > 1 ? `残り ${phase.remainingChunks} 区間` : undefined,
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
      return "権限がないためコピーのみ行いました";
    case "secureField":
      return "入力先がパスワード欄だったので、貼り付けていません";
    case "pasteFailed":
      return "自動入力できなかったためコピーしました";
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
      return { title: "音声認識モデルがありません", detail: "設定画面からダウンロードしてください" };
    case "micPermission":
      return { title: "マイクの使用が許可されていません", detail: micSettingsPath(platform) };
    case "micUnavailable":
      return { title: "マイクを開けませんでした", detail: "入力デバイスを確認してください" };
    case "micStartTimedOut":
      return { title: "マイクの起動が応答しません", detail: "もう一度押すと再試行します" };
    case "micLost":
      return { title: "録音中にマイクが切断されました" };
    case "noAudio":
      return {
        title: `${failure.detail} から音声が届きませんでした`,
        detail: "ミュートや入力デバイスの設定を確認してください（ノートPCは蓋を閉じると内蔵マイクが止まります）",
      };
    case "tooShort":
      return { title: "録音が短くて、文字にできませんでした" };
    case "nothingRecognized":
      return { title: "話し声が聞き取れませんでした" };
    case "transcription":
      return { title: "文字にする途中で止まりました", detail: "設定画面の状態を確認してください" };
  }
}
