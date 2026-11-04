// First-run setup: download what is missing, grant macOS permissions, then try the shortcut once.

import "../shared/base.css";
import "../shared/ui.css";
import "./setup.css";

import { api, errorText, events } from "../shared/api";
import { runDownload } from "../shared/download";
import { formatBytes, formatMemory, installCommand, purposeLabel, toolLabel } from "../shared/format";
import { icons } from "../shared/icons";
import { phaseMessage } from "../shared/messages";
import { mountShortcutPicker } from "../shared/shortcut-picker";
import type { ModelKind, Settings, SetupStatus, Tool, UiContext } from "../shared/types";

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

type Step = "download" | "permissions" | "try";

let ctx: UiContext;
let settings: Settings;
let status: SetupStatus;
let steps: Step[];
let current = 0;
let downloading = false;

// ---------------------------------------------------------------- navigation

function show(index: number): void {
  current = index;
  steps.forEach((step, i) => (el(`step-${step}`).hidden = i !== index));
  el("steps").replaceChildren(
    ...steps.map((_, i) => {
      const dot = document.createElement("span");
      if (i <= index) dot.className = "done";
      return dot;
    }),
  );
  el("step-count").textContent = `${index + 1} / ${steps.length}`;
  if (steps[index] === "try") el("try-text").focus();
}

// ---------------------------------------------------------------- download

function statusRow(ok: boolean, title: string, note: string): HTMLLIElement {
  const li = document.createElement("li");
  const icon = document.createElement("span");
  icon.className = ok ? "icon ok" : "icon";
  icon.innerHTML = ok ? icons.check : icons.download;
  const what = document.createElement("div");
  what.className = "what";
  const t = document.createElement("span");
  t.textContent = title;
  const n = document.createElement("span");
  n.className = "muted small";
  n.textContent = note;
  what.append(t, n);
  li.append(icon, what);
  return li;
}

function tierName(file: string): string {
  const entry = status.catalog.find((c) => c.file === file);
  return entry?.tier === "light" ? "軽量モデル" : "標準モデル";
}

function renderDownload(): void {
  const purposes: ModelKind[] = status.memoryMode === "critical" ? ["whisper"] : ["whisper", "llm"];
  const rows = purposes.map((purpose) => {
    const planned = status.plan.downloads.filter((d) => d.purpose === purpose);
    const missingTool = status.plan.manual.includes(purpose === "whisper" ? "whisper" : "llama");
    if (planned.length === 0 && !missingTool) return statusRow(true, purposeLabel[purpose], "準備完了");
    const model = planned.find((d) => d.destination === "models");
    const bytes = planned.reduce((sum, d) => sum + d.bytes, 0);
    const note = [model ? tierName(model.file) : null, bytes > 0 ? formatBytes(bytes) : null, missingTool ? "ツールの手動インストールが必要" : null]
      .filter(Boolean)
      .join("・");
    return statusRow(false, purposeLabel[purpose], note);
  });
  el("download-items").replaceChildren(...rows);

  const hasModels = status.plan.downloads.some((d) => d.destination === "models");
  el("download-note").textContent =
    status.memoryMode === "critical"
      ? "利用可能なメモリが少ないため、文章校正機能はスキップされます。"
      : hasModels
        ? `お使いの環境の空きメモリ（${formatMemory(status.availableMemoryMb)}）に合わせて最適なモデルを選択しました。後から設定の「詳細」で変更できます。`
        : "";

  el("manual").replaceChildren(...status.plan.manual.map(manualCard));

  const pending = status.plan.downloads.length > 0;
  const start = el<HTMLButtonElement>("download-start");
  start.hidden = !pending;
  start.disabled = downloading;
  start.textContent = `ダウンロードして始める（${formatBytes(status.planBytes)}）`;
  el("download-next").hidden = pending || !status.ready;
}

function manualCard(tool: Tool): HTMLLIElement {
  const li = document.createElement("li");
  li.className = "card status";
  const icon = document.createElement("span");
  icon.className = "icon none";
  icon.innerHTML = icons.warning;
  const body = document.createElement("div");
  body.className = "body";
  const title = document.createElement("div");
  title.className = "title";
  title.textContent = toolLabel[tool];
  const note = document.createElement("div");
  note.className = "note";
  const command = installCommand(ctx.platform, tool);
  note.textContent = command
    ? "お使いの環境向けの自動ダウンロードが用意されていないため、ターミナルで以下のコマンドを実行してください。完了後にこの画面へ戻ると、自動的に再認識されます。"
    : "お使いの環境向けの自動ダウンロードが用意されていません。whisper.cpp の Windows 版をインストールし、PATH（環境変数）が通った場所に配置してください。";
  body.append(title, note);
  if (command) {
    const row = document.createElement("div");
    row.className = "command";
    const code = document.createElement("code");
    code.textContent = command;
    const copy = document.createElement("button");
    copy.type = "button";
    copy.textContent = "コピー";
    copy.addEventListener("click", () => {
      void navigator.clipboard.writeText(command).then(() => (copy.textContent = "コピーしました"));
    });
    row.append(code, copy);
    body.append(row);
  }
  const tag = document.createElement("span");
  tag.className = "tag";
  tag.textContent = tool === "whisper" ? "必須" : "任意";
  li.append(icon, body, tag);
  return li;
}

async function startDownload(): Promise<void> {
  downloading = true;
  el("download-error").textContent = "";
  el<HTMLButtonElement>("download-start").hidden = true;
  try {
    const finished = await runDownload("setup", {
      box: el("download-box"),
      bar: el("download-bar"),
      label: el("download-label"),
      cancel: el<HTMLButtonElement>("download-cancel"),
    });
    await refresh();
    if (finished && status.ready) show(current + 1);
  } catch (e) {
    el("download-error").textContent = errorText(e);
  } finally {
    downloading = false;
    await refresh();
  }
}

// ---------------------------------------------------------------- permissions

function permissionCard(
  card: HTMLElement,
  ok: boolean,
  title: string,
  note: string,
  tag: string,
  action?: { label: string; primary?: boolean; run: () => Promise<unknown> },
  steps?: string[],
): void {
  const icon = document.createElement("span");
  icon.className = ok ? "icon ok" : "icon todo";
  icon.innerHTML = ok ? icons.check : icons.info;
  const body = document.createElement("div");
  body.className = "body";
  const t = document.createElement("div");
  t.className = "title";
  t.textContent = title;
  const n = document.createElement("div");
  n.className = "note";
  n.textContent = note;
  body.append(t, n);
  if (!ok && steps) {
    const ol = document.createElement("ol");
    ol.className = "small";
    ol.replaceChildren(
      ...steps.map((s) => {
        const li = document.createElement("li");
        li.textContent = s;
        return li;
      }),
    );
    body.append(ol);
  }
  if (!ok && action) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = action.primary ? "primary" : "";
    button.textContent = action.label;
    button.addEventListener("click", () => void action.run().then(refresh));
    const row = document.createElement("div");
    row.append(button);
    body.append(row);
  }
  const chip = document.createElement("span");
  chip.className = "tag";
  chip.textContent = tag;
  card.replaceChildren(icon, body, chip);
}

function renderPermissions(): void {
  const mic = status.microphonePermission;
  permissionCard(
    el("mic-status"),
    mic === "granted",
    "マイク",
    mic === "granted"
      ? "アクセスが許可されています。"
      : mic === "notDetermined"
        ? "音声の録音に必要です。ボタンを押すと macOS の確認ダイアログが表示されます。"
        : "アクセスが許可されていません。システム設定の「プライバシーとセキュリティ」>「マイク」で Kotonoha Voice を有効にしてください。",
    "必須",
    mic === "notDetermined"
      ? { label: "マイクを許可する", primary: true, run: api.requestMicrophone }
      : { label: "システム設定を開く", run: api.openMicrophoneSettings },
  );
  permissionCard(
    el("input-status"),
    status.inputPermission,
    "アクセシビリティ（テキスト自動入力）",
    status.inputPermission
      ? "アクセスが許可されています。"
      : `許可しない場合、テキストはクリップボードにのみコピーされます（${ctx.pasteLabel} で貼り付け可能）。`,
    "任意",
    { label: "システム設定を開く", run: api.openInputPermissionSettings },
    ["「システム設定を開く」をクリックする", "リスト内の「Kotonoha Voice」を有効にする", "この画面に戻る（自動で再チェックされます）"],
  );
}

// ---------------------------------------------------------------- try

const trialTone = { neutral: "", recording: "", busy: "", success: "success", info: "warning", warning: "warning" };

async function renderTry(): Promise<void> {
  ctx = await api.uiContext();
  el("try-how").textContent = ctx.holdShortcutLabel
    ? `${ctx.shortcutLabel} を押して話し、もう一度押すとテキストが入力されます。${ctx.holdShortcutLabel} は押している間だけ録音し、離すとテキストが入力されます。`
    : `${ctx.shortcutLabel} を押して話し、もう一度押すとテキストが入力されます。`;
  el("try-hint").textContent =
    "下の入力欄にカーソルを置いてお試しください。普段お使いのアプリでも、同様にカーソル位置へ直接入力されます。";
}

function showPhase(phase: Parameters<typeof phaseMessage>[0]): void {
  const line = el("try-status");
  if (phase.phase === "idle") return;
  const message = phaseMessage(phase, ctx);
  line.className = `status-line ${trialTone[message.tone]}`;
  line.textContent = message.detail ? `${message.title}（${message.detail}）` : message.title;
}

// ---------------------------------------------------------------- start

async function refresh(): Promise<void> {
  status = await api.setupStatus();
  renderDownload();
  if (steps.includes("permissions")) renderPermissions();
}

async function main(): Promise<void> {
  [ctx, settings] = await Promise.all([api.uiContext(), api.getSettings()]);
  el("app-version").textContent = `バージョン ${ctx.version}`;
  steps = ctx.platform === "macOS" ? ["download", "permissions", "try"] : ["download", "try"];
  await refresh();

  const picker = mountShortcutPicker(
    el("shortcut-choices"),
    el("shortcut-error"),
    ctx.platform,
    () => settings.shortcut,
    async (shortcut) => {
      await api.saveSettings({ ...settings, shortcut: shortcut! });
      settings = await api.getSettings();
      await renderTry();
      await holdPicker.render();
    },
  );
  const holdPicker = mountShortcutPicker(
    el("hold-choices"),
    el("hold-error"),
    ctx.platform,
    () => settings.holdShortcut,
    async (holdShortcut) => {
      await api.saveSettings({ ...settings, holdShortcut });
      settings = await api.getSettings();
      await renderTry();
    },
    { optional: true },
  );
  await Promise.all([picker.render(), holdPicker.render(), renderTry()]);

  el("download-start").addEventListener("click", () => void startDownload());
  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-action]")) {
    const action = button.dataset.action;
    button.addEventListener("click", () => {
      if (action === "next") show(Math.min(current + 1, steps.length - 1));
      if (action === "back") show(Math.max(current - 1, 0));
      if (action === "later") void api.finishSetup();
    });
  }
  el("finish").addEventListener("click", () => void api.finishSetup());
  await events.onPhase(showPhase);
  // Tools installed in a terminal and permissions granted in System Settings show up on return
  window.addEventListener("focus", () => void (downloading ? undefined : refresh()));

  show(0);
}

void main();
