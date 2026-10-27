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
    if (planned.length === 0 && !missingTool) return statusRow(true, purposeLabel[purpose], "準備できています");
    const model = planned.find((d) => d.destination === "models");
    const bytes = planned.reduce((sum, d) => sum + d.bytes, 0);
    const note = [model ? tierName(model.file) : null, bytes > 0 ? formatBytes(bytes) : null, missingTool ? "ツールは手動で入れます" : null]
      .filter(Boolean)
      .join("・");
    return statusRow(false, purposeLabel[purpose], note);
  });
  el("download-items").replaceChildren(...rows);

  const hasModels = status.plan.downloads.some((d) => d.destination === "models");
  el("download-note").textContent =
    status.memoryMode === "critical"
      ? "空きメモリが少ないため、文章の整えは使いません。"
      : hasModels
        ? `空きメモリ ${formatMemory(status.availableMemoryMb)} に合わせて選びました。あとで設定の「詳細」から変えられます。`
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
    ? "このコンピューター向けのダウンロードがないため、ターミナルで次のコマンドを実行してください。終わったらこの画面に戻ると、自動で確認し直します。"
    : "このコンピューター向けのダウンロードがありません。whisper.cpp の Windows 版を入れ、PATH の通った場所に置いてください。";
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
      ? "許可されています。"
      : mic === "notDetermined"
        ? "録音に必要です。ボタンを押すと macOS の確認画面が出ます。"
        : "許可されていません。システム設定の「プライバシーとセキュリティ」>「マイク」で Kotonoha をオンにしてください。",
    "必須",
    mic === "notDetermined"
      ? { label: "マイクを許可する", primary: true, run: api.requestMicrophone }
      : { label: "システム設定を開く", run: api.openMicrophoneSettings },
  );
  permissionCard(
    el("input-status"),
    status.inputPermission,
    "アクセシビリティ（自動で貼り付けるため）",
    status.inputPermission
      ? "許可されています。"
      : `許可しない場合は、文字をクリップボードにコピーするだけになります。${ctx.pasteLabel} で貼り付けてください。`,
    "任意",
    { label: "システム設定を開く", run: api.openInputPermissionSettings },
    ["ボタンでシステム設定を開く", "一覧の Kotonoha をオンにする", "この画面に戻る（自動で確認します）"],
  );
}

// ---------------------------------------------------------------- try

const trialTone = { neutral: "", recording: "", busy: "", success: "success", info: "warning", warning: "warning" };

async function renderTry(): Promise<void> {
  ctx = await api.uiContext();
  el("try-how").textContent = `${ctx.shortcutLabel} を一度押して話し、もう一度押すと文字になります。押したまま話して離しても同じです。`;
  el("try-hint").textContent =
    "この欄にカーソルを置いて試してください。ほかのアプリでも、カーソルのある場所に同じように入ります。";
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
  steps = ctx.platform === "macOS" ? ["download", "permissions", "try"] : ["download", "try"];
  await refresh();

  const picker = mountShortcutPicker(
    el("shortcut-choices"),
    el("shortcut-error"),
    ctx.platform,
    () => settings.shortcut,
    async (shortcut) => {
      await api.saveSettings({ ...settings, shortcut });
      settings = await api.getSettings();
      await renderTry();
    },
  );
  await Promise.all([picker.render(), renderTry()]);

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
