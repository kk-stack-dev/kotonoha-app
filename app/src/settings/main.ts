// The settings window: 一般 (shortcut and behaviour), 辞書 (the dictionary) and 詳細 (models and info).
// Every change is saved at once; a short "保存しました" next to the section heading confirms it.

import "../shared/base.css";
import "../shared/ui.css";
import "./settings.css";

import { api, errorText, events, isCancelled } from "../shared/api";
import { runDownload } from "../shared/download";
import { fileName, formatBytes, formatMemory, modelLabel } from "../shared/format";
import { icons } from "../shared/icons";
import { mountShortcutPicker } from "../shared/shortcut-picker";
import type {
  Dictionary,
  DictionaryEntry,
  FoundComputer,
  LanStatus,
  ModelKind,
  Settings,
  SetupStatus,
  UiContext,
} from "../shared/types";

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let ctx: UiContext;
let settings: Settings;
let status: SetupStatus;
let lan: LanStatus;

// ---------------------------------------------------------------- tabs

const TABS = ["general", "dictionary", "lan", "advanced"] as const;

function selectTab(name: (typeof TABS)[number], focus = false): void {
  for (const tab of TABS) {
    const button = el(`tab-${tab}`);
    const selected = tab === name;
    button.setAttribute("aria-selected", String(selected));
    button.tabIndex = selected ? 0 : -1;
    el(`panel-${tab}`).hidden = !selected;
    if (selected && focus) button.focus();
  }
}

function setUpTabs(): void {
  TABS.forEach((tab, i) => {
    const button = el(`tab-${tab}`);
    button.addEventListener("click", () => selectTab(tab));
    button.addEventListener("keydown", (e) => {
      const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
      if (step) selectTab(TABS[(i + step + TABS.length) % TABS.length]!, true);
    });
  });
}

// ---------------------------------------------------------------- saving

const savedTimers = new Map<string, number>();

function showSaved(section: string): void {
  const badge = document.querySelector<HTMLElement>(`[data-saved="${section}"]`);
  if (!badge) return;
  badge.innerHTML = `${icons.check}保存しました`;
  badge.classList.add("show");
  window.clearTimeout(savedTimers.get(section));
  savedTimers.set(
    section,
    window.setTimeout(() => badge.classList.remove("show"), 2000),
  );
}

async function save(change: Partial<Settings>, section: string, errorId: string): Promise<boolean> {
  try {
    await api.saveSettings({ ...settings, ...change });
    settings = await api.getSettings();
    el(errorId).textContent = "";
    showSaved(section);
    return true;
  } catch (e) {
    el(errorId).textContent = errorText(e);
    return false;
  }
}

// ---------------------------------------------------------------- banners

function banner(title: string, note: string, action: string, run: () => unknown): HTMLElement {
  const box = document.createElement("div");
  box.className = "banner";
  box.setAttribute("role", "alert");
  const icon = document.createElement("span");
  icon.innerHTML = icons.warning;
  const text = document.createElement("div");
  text.className = "text";
  const strong = document.createElement("strong");
  strong.textContent = title;
  const small = document.createElement("span");
  small.className = "small";
  small.textContent = note;
  text.append(strong, small);
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = action;
  button.addEventListener("click", () => void run());
  box.append(icon, text, button);
  return box;
}

/** Problems that stop dictation, and only those; nothing shows when all is well. */
function renderBanners(): void {
  const list: HTMLElement[] = [];
  if (!status.ready && !sendsWork()) {
    list.push(banner("初期セットアップが未完了です", "音声認識に必要なモデルファイルが見つかりません。", "セットアップを開く", api.openSetup));
  }
  if (status.ready && status.plan.downloads.some((d) => d.destination === "tools")) {
    list.push(
      banner(
        "処理用ツールの新しい版があります",
        "お使いの環境により適した版（GPU を使う版など）に入れ替えられます。セットアップ画面からダウンロードしてください。",
        "セットアップを開く",
        api.openSetup,
      ),
    );
  }
  if (status.microphonePermission === "denied") {
    list.push(banner("マイクへのアクセスが許可されていません", "音声を録音するにはマイクのアクセス許可が必要です。", "システム設定を開く", api.openMicrophoneSettings));
  }
  if (ctx.platform === "macOS" && settings.autoInsert && !status.inputPermission) {
    list.push(
      banner(
        "テキストを自動入力できません",
        `アクセシビリティ権限がないため、クリップボードへのコピーのみ行われます（${ctx.pasteLabel} で貼り付け可能）。`,
        "システム設定を開く",
        api.openInputPermissionSettings,
      ),
    );
  }
  el("banners").replaceChildren(...list);
}

// ---------------------------------------------------------------- general

interface SwitchDef {
  id: string;
  key: "autoInsert" | "keepOnClipboard" | "removeFillers" | "lineBreaks" | "llmRewrite" | "checkUpdatesOnLaunch";
  section: string;
  errorId: string;
}

const SWITCHES: SwitchDef[] = [
  { id: "auto-insert", key: "autoInsert", section: "insert", errorId: "insert-error" },
  { id: "keep-clipboard", key: "keepOnClipboard", section: "insert", errorId: "insert-error" },
  { id: "remove-fillers", key: "removeFillers", section: "formatting", errorId: "format-error" },
  { id: "line-breaks", key: "lineBreaks", section: "formatting", errorId: "format-error" },
  { id: "llm-rewrite", key: "llmRewrite", section: "formatting", errorId: "format-error" },
  { id: "check-updates", key: "checkUpdatesOnLaunch", section: "launch", errorId: "launch-error" },
];

function renderGeneral(): void {
  for (const item of SWITCHES) el<HTMLInputElement>(item.id).checked = settings[item.key];

  // Keeping the clipboard only matters when pasting
  const keep = el<HTMLInputElement>("keep-clipboard");
  keep.disabled = !settings.autoInsert;
  el("keep-clipboard-row").classList.toggle("off", !settings.autoInsert);
  el("keep-clipboard-row").classList.toggle("disabled", !settings.autoInsert);

  // Clean-up needs llama.cpp and a model; say which is missing instead of a switch that does nothing
  const llm = el<HTMLInputElement>("llm-rewrite");
  const missing = sendsWork()
    ? null
    : status.memoryMode === "critical"
      ? "空きメモリが不足しているため、現在は利用できません"
      : !status.llamaTool
        ? "llama.cpp がインストールされていないため利用できません"
        : !status.models.some((m) => m.kind === "llm")
          ? "文章校正モデルがダウンロードされていないため利用できません"
          : null;
  llm.disabled = missing !== null;
  if (missing) llm.checked = false;
  el("llm-row").classList.toggle("off", missing !== null);
  el("llm-row").classList.toggle("disabled", missing !== null);
  el("llm-note").textContent =
    missing ??
    (sendsWork()
      ? "ペアリングしたコンピューターのAI（言語モデル）で自然な文章に整えます"
      : "端末内のローカルAI（言語モデル）で自然な文章に整えます");
}

// ---------------------------------------------------------------- dictionary

let dictionary: Dictionary;
let removed: DictionaryEntry | null = null;
let toastTimer = 0;
let editing: string | null = null;

function wordRow(entry: DictionaryEntry): HTMLLIElement {
  const li = document.createElement("li");
  if (editing === entry.word) return editRow(li, entry);
  const word = document.createElement("span");
  word.className = "word";
  word.textContent = entry.word;
  const variants = document.createElement("span");
  variants.className = "variants";
  variants.textContent = entry.variants.join("、");
  li.append(word, variants);

  if (entry.source === "builtin") {
    li.classList.toggle("off", !entry.enabled);
    const toggle = document.createElement("input");
    toggle.type = "checkbox";
    toggle.setAttribute("role", "switch");
    toggle.checked = entry.enabled;
    toggle.setAttribute("aria-label", `${entry.word} を使う`);
    toggle.addEventListener("change", () => void dictionaryChange(api.setWordEnabled(entry.word, toggle.checked)));
    li.append(toggle);
    return li;
  }
  const edit = document.createElement("button");
  edit.type = "button";
  edit.className = "link";
  edit.textContent = "編集";
  edit.setAttribute("aria-label", `${entry.word} を編集`);
  edit.addEventListener("click", () => {
    editing = entry.word;
    renderDictionary();
    document.querySelector<HTMLInputElement>("li.editing input")?.focus();
  });
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "quiet";
  remove.textContent = "削除";
  remove.setAttribute("aria-label", `${entry.word} を削除`);
  remove.addEventListener("click", () => void removeWord(entry));
  li.append(edit, remove);
  return li;
}

function editRow(li: HTMLLIElement, entry: DictionaryEntry): HTMLLIElement {
  li.className = "editing";
  const word = document.createElement("input");
  word.className = "word";
  word.value = entry.word;
  word.setAttribute("aria-label", "正しい書き方");
  const variants = document.createElement("input");
  variants.className = "variants";
  variants.value = entry.variants.join("、");
  variants.setAttribute("aria-label", "間違えやすい書き方");
  const saveButton = document.createElement("button");
  saveButton.type = "button";
  saveButton.className = "primary";
  saveButton.textContent = "保存";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "quiet";
  cancel.textContent = "キャンセル";
  const commit = async () => {
    if (await dictionaryChange(api.editWord(entry.word, word.value, variants.value))) {
      editing = null;
      renderDictionary();
    }
  };
  const stop = () => {
    editing = null;
    renderDictionary();
  };
  saveButton.addEventListener("click", () => void commit());
  cancel.addEventListener("click", stop);
  for (const input of [word, variants]) {
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") void commit();
      if (e.key === "Escape") stop();
    });
  }
  li.append(word, variants, saveButton, cancel);
  return li;
}

function renderDictionary(): void {
  const mine = dictionary.entries.filter((e) => e.source === "user");
  const builtin = dictionary.entries.filter((e) => e.source === "builtin");
  el("mine-heading").textContent = `登録済み単語（${mine.length}）`;
  el("user-words").replaceChildren(...mine.map(wordRow));
  el("user-empty").hidden = mine.length > 0;
  const off = builtin.filter((e) => !e.enabled).length;
  el("builtin-heading").textContent = `標準辞書（${builtin.length}${off > 0 ? `、うち ${off} 件無効` : ""}）`;
  el("builtin-words").replaceChildren(...builtin.map(wordRow));
}

/** Runs a dictionary command; shows its error and returns false if it failed. */
async function dictionaryChange(request: Promise<Dictionary>): Promise<boolean> {
  try {
    dictionary = await request;
    el("dictionary-error").textContent = "";
    renderDictionary();
    return true;
  } catch (e) {
    el("dictionary-error").textContent = errorText(e);
    return false;
  }
}

async function addWord(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const word = el<HTMLInputElement>("new-word");
  const variants = el<HTMLInputElement>("new-variants");
  if (await dictionaryChange(api.addWord(word.value, variants.value))) {
    word.value = "";
    variants.value = "";
    word.focus();
  }
}

async function removeWord(entry: DictionaryEntry): Promise<void> {
  if (!(await dictionaryChange(api.removeWord(entry.word)))) return;
  removed = entry;
  el("toast-text").textContent = `「${entry.word}」を削除しました`;
  el("toast").hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el("toast").hidden = true), 6000);
}

async function undoRemove(): Promise<void> {
  el("toast").hidden = true;
  if (!removed) return;
  const entry = removed;
  removed = null;
  await dictionaryChange(api.addWord(entry.word, entry.variants.join("、")));
}

// ---------------------------------------------------------------- LAN

/** Work goes to a paired computer. */
function sendsWork(): boolean {
  return settings.lan.useRemote && lan?.server != null;
}

function remotePermissionNote(): string {
  return ctx.platform === "macOS"
    ? "続けると、macOS が「ローカルネットワーク上のデバイスの検出」の許可を求めます。「許可」を選んでください。"
    : "Windows のファイアウォールが許可を求めた場合は、「プライベート ネットワーク」だけを許可してください。";
}

function servePermissionNote(): string {
  return ctx.platform === "macOS"
    ? "続けると、macOS がローカルネットワークへのアクセスの許可を求めます。「許可」を選んでください。"
    : "続けると、Windows のファイアウォールが許可を求めます。「プライベート ネットワーク」だけを許可してください。パブリック ネットワークにつながっている間は受け付けません。";
}

function serveStateText(): string {
  switch (lan.problem) {
    case "publicNetwork":
      return "パブリック ネットワークにつながっているため、受け付けを止めています。自宅や職場のネットワークでは、Windows の設定でネットワークの種類を「プライベート」にしてください。";
    case "portInUse":
      return `ポート 50540 をほかのアプリが使っているため、受け付けを始められません。`;
    case "failed":
      return "受け付けを始められませんでした。ログを確認してください。";
    case null:
      break;
  }
  if (!lan.serving) return "準備しています...";
  const addresses = lan.addresses.length > 0 ? `（IP アドレス: ${lan.addresses.join("、")}）` : "";
  const ready = status.ready ? "" : "ただし、このコンピューターに音声認識のモデルがないため、まだ処理できません。";
  return `受け付けています。このコンピューターの名前は「${lan.computerName}」です${addresses}。${ready}`;
}

let pairing = false;
let browsing = false;
let found: FoundComputer[] = [];

function listItem(name: string, action: string, label: string, run: () => unknown): HTMLLIElement {
  const li = document.createElement("li");
  const text = document.createElement("span");
  text.className = "name";
  text.textContent = name;
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = action;
  button.setAttribute("aria-label", label);
  button.addEventListener("click", () => void run());
  li.append(text, button);
  return li;
}

function renderLan(): void {
  const remoteOn = settings.lan.useRemote;
  if (el("remote-confirm").hidden) el<HTMLInputElement>("use-remote").checked = remoteOn;
  el("remote-body").hidden = !remoteOn;
  el("remote-note").textContent = !remoteOn
    ? "オンにすると、同じ LAN で処理を受け付けているコンピューターを探します"
    : lan.server
      ? `「${lan.server.name}」で処理します`
      : "ペアリングするまでは、このコンピューターで処理します";
  el<HTMLInputElement>("fallback-local").checked = settings.lan.fallbackToLocal;
  el("paired-server").hidden = !lan.server;
  el("paired-name").textContent = lan.server?.name ?? "";
  el("finder").hidden = !!lan.server || pairing;
  el("browse-again").toggleAttribute("disabled", browsing);
  el("found-list").replaceChildren(
    ...found.map((c) =>
      listItem(c.name || c.addresses[0] || "", "ペアリング", `${c.name} とペアリング`, () => pairWith(c.addresses, c.name)),
    ),
  );

  const serveOn = settings.lan.serve;
  if (el("serve-confirm").hidden) el<HTMLInputElement>("serve").checked = serveOn;
  el("serve-body").hidden = !serveOn;
  el("serve-state").textContent = serveOn ? serveStateText() : "";
  const request = lan.pairRequest;
  el("pair-request").hidden = !request;
  if (request) {
    el("request-title").textContent = `「${request.name}」がペアリングを求めています`;
    el("request-code").textContent = request.code;
  }
  el("client-list").replaceChildren(
    ...lan.clients.map((c) =>
      listItem(c.name, "解除", `${c.name} とのペアリングを解除`, async () => {
        await lanAction(api.lanForgetClient(c.key), "serve-error");
      }),
    ),
  );
  el("client-empty").hidden = lan.clients.length > 0;
}

async function refreshLan(): Promise<void> {
  lan = await api.lanStatus();
  renderLan();
  renderGeneral();
  renderBanners();
}

async function lanAction(request: Promise<unknown>, errorId: string): Promise<boolean> {
  try {
    await request;
    el(errorId).textContent = "";
    await refreshLan();
    return true;
  } catch (e) {
    el(errorId).textContent = errorText(e);
    return false;
  }
}

/** Shows the remote error, with a way to the permission when macOS may have refused it. */
function showRemoteError(message: string): void {
  el("remote-error").textContent = message;
  el("local-network-help").hidden = !(ctx.platform === "macOS" && message.includes("ローカルネットワーク"));
}

async function browse(): Promise<void> {
  if (browsing) return;
  browsing = true;
  found = [];
  showRemoteError("");
  el("found-note").textContent = "探しています...";
  renderLan();
  try {
    found = await api.lanBrowse();
    el("found-note").textContent =
      found.length > 0
        ? ""
        : "見つかりませんでした。相手のコンピューターで「ペアリングしたコンピューターの処理を受け付ける」をオンにしてから、もう一度探してください。";
  } catch (e) {
    el("found-note").textContent = "";
    showRemoteError(errorText(e));
  } finally {
    browsing = false;
    renderLan();
  }
}

async function pairWith(addresses: string[], name: string): Promise<void> {
  pairing = true;
  showRemoteError("");
  el("check-result").textContent = "";
  el("pairing").hidden = false;
  el("pairing-title").textContent = name ? `「${name}」に接続しています` : "接続しています";
  el("pairing-code").textContent = "";
  el("pairing-note").textContent = "";
  renderLan();
  try {
    const server = await api.lanPair(addresses);
    el("check-result").textContent = `「${server.name}」とペアリングしました。これからの録音は、「${server.name}」で処理します。`;
  } catch (e) {
    if (!isCancelled(e)) showRemoteError(errorText(e));
  } finally {
    pairing = false;
    el("pairing").hidden = true;
    await refreshLan();
  }
}

function showPairingCode(code: string): void {
  if (!pairing) return;
  el("pairing-title").textContent = "確認番号";
  el("pairing-code").textContent = code;
  el("pairing-note").textContent =
    "相手のコンピューターに同じ番号が出ていることを確かめて、相手の画面で「許可」を押してください。";
}

async function checkServer(): Promise<void> {
  const button = el<HTMLButtonElement>("check-server");
  button.disabled = true;
  showRemoteError("");
  el("check-result").textContent = "接続しています...";
  try {
    el("check-result").textContent = await api.lanCheck();
  } catch (e) {
    el("check-result").textContent = "";
    showRemoteError(errorText(e));
  } finally {
    button.disabled = false;
  }
}

async function setRemote(on: boolean): Promise<void> {
  el("remote-confirm").hidden = true;
  if (!(await save({ lan: { ...settings.lan, useRemote: on } }, "remote", "remote-error"))) return;
  if (!on) {
    found = [];
    if (pairing) await api.lanPairCancel();
  }
  await refreshLan();
  if (on && !lan.server) await browse();
}

async function setServing(on: boolean): Promise<void> {
  el("serve-confirm").hidden = true;
  try {
    lan = await api.lanSetServing(on);
    settings = await api.getSettings();
    el("serve-error").textContent = "";
    showSaved("serve");
  } catch (e) {
    el("serve-error").textContent = errorText(e);
  }
  renderLan();
}

function setUpLan(): void {
  el("remote-permission").textContent = remotePermissionNote();
  el("serve-permission").textContent = servePermissionNote();
  el("manual-toggle").querySelector(".chevron")!.innerHTML = icons.chevron;

  // Turning a role on first explains what happens; the OS asks for its permission only after "続ける"
  const remote = el<HTMLInputElement>("use-remote");
  remote.addEventListener("change", () => {
    if (remote.checked) {
      remote.checked = false;
      el("remote-confirm").hidden = false;
      el<HTMLButtonElement>("remote-continue").focus();
    } else {
      void setRemote(false);
    }
  });
  el("remote-continue").addEventListener("click", () => void setRemote(true));
  el("remote-cancel").addEventListener("click", () => (el("remote-confirm").hidden = true));

  const serve = el<HTMLInputElement>("serve");
  serve.addEventListener("change", () => {
    if (serve.checked) {
      serve.checked = false;
      el("serve-confirm").hidden = false;
      el<HTMLButtonElement>("serve-continue").focus();
    } else {
      void setServing(false);
    }
  });
  el("serve-continue").addEventListener("click", () => void setServing(true));
  el("serve-cancel").addEventListener("click", () => (el("serve-confirm").hidden = true));

  const fallback = el<HTMLInputElement>("fallback-local");
  fallback.addEventListener("change", async () => {
    if (!(await save({ lan: { ...settings.lan, fallbackToLocal: fallback.checked } }, "remote", "remote-error"))) {
      fallback.checked = !fallback.checked;
    }
  });

  el("browse-again").addEventListener("click", () => void browse());
  const manualToggle = el("manual-toggle");
  manualToggle.addEventListener("click", () => {
    const open = manualToggle.getAttribute("aria-expanded") !== "true";
    manualToggle.setAttribute("aria-expanded", String(open));
    el("manual-box").hidden = !open;
    if (open) el<HTMLInputElement>("manual-address").focus();
  });
  el<HTMLFormElement>("manual-box").addEventListener("submit", (e) => {
    e.preventDefault();
    const address = el<HTMLInputElement>("manual-address").value.trim();
    if (address) void pairWith([address], "");
  });
  el("pairing-cancel").addEventListener("click", () => void api.lanPairCancel());
  el("check-server").addEventListener("click", () => void checkServer());
  el("forget-server").addEventListener("click", async () => {
    el("check-result").textContent = "";
    if (await lanAction(api.lanForgetServer(), "remote-error")) await browse();
  });
  el("open-local-network").addEventListener("click", () => void api.openLocalNetworkSettings());
  el("request-accept").addEventListener("click", () => void lanAction(api.lanPairAnswer(true), "serve-error"));
  el("request-decline").addEventListener("click", () => void lanAction(api.lanPairAnswer(false), "serve-error"));
}

// ---------------------------------------------------------------- advanced

function fillModelSelect(select: HTMLSelectElement, kind: ModelKind, chosen: string | null): void {
  const option = (value: string, text: string) => {
    const o = document.createElement("option");
    o.value = value;
    o.textContent = text;
    return o;
  };
  const files = status.models.filter((m) => m.kind === kind);
  const options = [option("", "自動選択（空きメモリに応じて最適化）")];
  options.push(...files.map((m) => option(m.path, modelLabel(m, status.catalog))));
  if (chosen && !files.some((m) => m.path === chosen)) options.push(option(chosen, `${fileName(chosen)}（見つかりません）`));
  select.replaceChildren(...options);
  select.value = chosen ?? "";
}

function renderAdvanced(): void {
  fillModelSelect(el("whisper-model"), "whisper", settings.models.whisper);
  fillModelSelect(el("llm-model"), "llm", settings.models.llm);
  const light = status.catalog.filter((m) => m.tier === "light" && !m.present);
  el("light-models").hidden = light.length === 0 || !el("light-box").hidden;
  el("light-note").textContent = `軽量モデルは未ダウンロードです（合計 ${formatBytes(light.reduce((s, m) => s + m.bytes, 0))}）`;
  el<HTMLSelectElement>("max-minutes").value = String(Math.round(settings.maxRecordingSeconds / 60));
  el("models-dir").textContent = status.modelsDir;
  el("memory").textContent = formatMemory(status.availableMemoryMb);
  el("version").textContent = ctx.version;
}

let updateAvailable = false;

function showUpdateAvailable(latest: string): void {
  updateAvailable = true;
  el("update-result").textContent = `最新バージョン ${latest} が利用可能です`;
  el("update-action").textContent = "リリースページを開く";
  el("update-now").hidden = false;
}

function setUpdateConfirm(open: boolean): void {
  el("update-confirm").hidden = !open;
  el("update-now").hidden = open || !updateAvailable;
}

/** Starts the update. On success the app quits and restarts, so this normally never returns. */
async function runUpdate(): Promise<void> {
  const run = el<HTMLButtonElement>("update-run");
  const cancel = el<HTMLButtonElement>("update-cancel");
  run.disabled = true;
  cancel.disabled = true;
  el("update-result").textContent = "更新しています。終わると自動で起動し直します";
  try {
    await api.startUpdate();
    el("update-result").textContent = "更新しました。起動し直してください";
  } catch (e) {
    el("update-result").textContent = errorText(e);
  } finally {
    run.disabled = false;
    cancel.disabled = false;
    setUpdateConfirm(false);
  }
}

/** Checks for a new version only when asked; when one exists, the same button opens the release page. */
async function onUpdateAction(): Promise<void> {
  if (updateAvailable) {
    await api.openReleasePage();
    return;
  }
  const button = el<HTMLButtonElement>("update-action");
  button.disabled = true;
  el("update-result").textContent = "アップデートを確認中...";
  try {
    const result = await api.checkUpdate();
    if (result.kind === "available") {
      showUpdateAvailable(result.latest);
    } else {
      el("update-result").textContent = "最新バージョンを使用中です";
    }
  } catch (e) {
    el("update-result").textContent = errorText(e);
  } finally {
    button.disabled = false;
  }
}

async function downloadLight(): Promise<void> {
  el("light-models").hidden = true;
  el("models-error").textContent = "";
  try {
    await runDownload("lightModels", {
      box: el("light-box"),
      bar: el("light-bar"),
      label: el("light-label"),
      cancel: el<HTMLButtonElement>("light-cancel"),
    });
  } catch (e) {
    el("models-error").textContent = errorText(e);
  }
  await refresh();
}

// ---------------------------------------------------------------- start

async function refresh(): Promise<void> {
  status = await api.setupStatus();
  renderBanners();
  renderGeneral();
  renderAdvanced();
}

async function main(): Promise<void> {
  [ctx, settings, dictionary, lan] = await Promise.all([
    api.uiContext(),
    api.getSettings(),
    api.getDictionary(),
    api.lanStatus(),
  ]);
  el("app-version").textContent = `バージョン ${ctx.version}`;
  setUpTabs();
  setUpLan();
  // Opened for a pairing request: show it right away
  if (lan.pairRequest) selectTab("lan");
  el("builtin-toggle").querySelector(".chevron")!.innerHTML = icons.chevron;

  const picker = mountShortcutPicker(
    el("shortcut-choices"),
    el("shortcut-error"),
    ctx.platform,
    () => settings.shortcut,
    async (shortcut) => {
      await api.saveSettings({ ...settings, shortcut: shortcut! });
      settings = await api.getSettings();
      ctx = await api.uiContext();
      showSaved("shortcut");
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
      ctx = await api.uiContext();
      showSaved("hold");
      await picker.render();
    },
    { optional: true },
  );

  for (const item of SWITCHES) {
    const input = el<HTMLInputElement>(item.id);
    input.addEventListener("change", async () => {
      if (!(await save({ [item.key]: input.checked }, item.section, item.errorId))) input.checked = !input.checked;
      renderGeneral();
      renderBanners();
    });
  }
  const autostart = el<HTMLInputElement>("autostart");
  autostart.checked = await api.getAutostart();
  autostart.addEventListener("change", async () => {
    try {
      await api.setAutostart(autostart.checked);
      el("launch-error").textContent = "";
      showSaved("launch");
    } catch (e) {
      el("launch-error").textContent = errorText(e);
      autostart.checked = !autostart.checked;
    }
  });

  el<HTMLFormElement>("dictionary-form").addEventListener("submit", (e) => void addWord(e));
  el("toast-undo").addEventListener("click", () => void undoRemove());
  const toggle = el("builtin-toggle");
  toggle.addEventListener("click", () => {
    const open = toggle.getAttribute("aria-expanded") !== "true";
    toggle.setAttribute("aria-expanded", String(open));
    el("builtin-words").hidden = !open;
  });

  for (const kind of ["whisper", "llm"] as const) {
    const select = el<HTMLSelectElement>(`${kind}-model`);
    select.addEventListener("change", async () => {
      await save({ models: { ...settings.models, [kind]: select.value || null } }, "models", "models-error");
      await refresh();
    });
  }
  el<HTMLSelectElement>("max-minutes").addEventListener("change", (e) =>
    save({ maxRecordingSeconds: Number((e.target as HTMLSelectElement).value) * 60 }, "recording", "models-error"),
  );
  el("light-download").addEventListener("click", () => void downloadLight());
  el("open-models").addEventListener("click", () => void api.openModelsFolder());
  el("open-logs").addEventListener("click", () => void api.openLogs());
  const pending = await api.pendingUpdate();
  if (pending) showUpdateAvailable(pending);
  void events.onUpdateAvailable(showUpdateAvailable);
  el("update-now").addEventListener("click", () => setUpdateConfirm(true));
  el("update-cancel").addEventListener("click", () => setUpdateConfirm(false));
  el("update-run").addEventListener("click", () => void runUpdate());
  el("update-action").addEventListener("click", () => void onUpdateAction());
  el("redo-setup").addEventListener("click", () => void api.openSetup());

  // Permissions, tools and models can change outside this window; refresh when it regains focus
  window.addEventListener("focus", () => void refresh());
  await events.onLanChanged(() => void refreshLan());
  await events.onLanPairCode(showPairingCode);
  await events.onLanPairRequest((request) => {
    lan = { ...lan, pairRequest: request };
    renderLan();
    if (request) selectTab("lan");
  });
  await events.onSettingsChanged(async () => {
    settings = await api.getSettings();
    await Promise.all([refresh(), refreshLan(), picker.render(), holdPicker.render()]);
  });

  renderDictionary();
  await Promise.all([refresh(), picker.render(), holdPicker.render()]);
  renderLan();
  if (settings.lan.useRemote && !lan.server) void browse();
}

void main();
