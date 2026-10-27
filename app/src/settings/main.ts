// The settings window: 一般 (shortcut and behaviour), 辞書 (the dictionary) and 詳細 (models and info).
// Every change is saved at once; a short "保存しました" next to the section heading confirms it.

import "../shared/base.css";
import "../shared/ui.css";
import "./settings.css";

import { api, errorText, events } from "../shared/api";
import { runDownload } from "../shared/download";
import { fileName, formatBytes, formatMemory, modelLabel } from "../shared/format";
import { icons } from "../shared/icons";
import { mountShortcutPicker } from "../shared/shortcut-picker";
import type { Dictionary, DictionaryEntry, ModelKind, Settings, SetupStatus, UiContext } from "../shared/types";

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let ctx: UiContext;
let settings: Settings;
let status: SetupStatus;

// ---------------------------------------------------------------- tabs

const TABS = ["general", "dictionary", "advanced"] as const;

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
  if (!status.ready) {
    list.push(banner("準備が終わっていません", "文字起こしに使うファイルがまだありません。", "セットアップを開く", api.openSetup));
  }
  if (status.microphonePermission === "denied") {
    list.push(banner("マイクの許可が外れています", "このままでは録音できません。", "システム設定を開く", api.openMicrophoneSettings));
  }
  if (ctx.platform === "macOS" && settings.autoInsert && !status.inputPermission) {
    list.push(
      banner(
        "自動で貼り付けられません",
        `アクセシビリティの許可がないため、コピーだけになります。${ctx.pasteLabel} で貼り付けてください。`,
        "システム設定を開く",
        api.openInputPermissionSettings,
      ),
    );
  }
  el("banners").replaceChildren(...list);
}

// ---------------------------------------------------------------- general

const SWITCHES: [string, "autoInsert" | "keepOnClipboard" | "removeFillers" | "lineBreaks" | "llmRewrite"][] = [
  ["auto-insert", "autoInsert"],
  ["keep-clipboard", "keepOnClipboard"],
  ["remove-fillers", "removeFillers"],
  ["line-breaks", "lineBreaks"],
  ["llm-rewrite", "llmRewrite"],
];

function renderGeneral(): void {
  for (const [id, key] of SWITCHES) el<HTMLInputElement>(id).checked = settings[key];

  // Keeping the clipboard only matters when pasting
  const keep = el<HTMLInputElement>("keep-clipboard");
  keep.disabled = !settings.autoInsert;
  el("keep-clipboard-row").classList.toggle("off", !settings.autoInsert);
  el("keep-clipboard-row").classList.toggle("disabled", !settings.autoInsert);

  // Clean-up needs llama.cpp and a model; say which is missing instead of a switch that does nothing
  const llm = el<HTMLInputElement>("llm-rewrite");
  const missing =
    status.memoryMode === "critical"
      ? "空きメモリが少ないため、いまは使えません"
      : !status.llamaTool
        ? "llama.cpp が入っていないため使えません"
        : !status.models.some((m) => m.kind === "llm")
          ? "文章の整え用のモデルがないため使えません"
          : null;
  llm.disabled = missing !== null;
  if (missing) llm.checked = false;
  el("llm-row").classList.toggle("off", missing !== null);
  el("llm-row").classList.toggle("disabled", missing !== null);
  el("llm-note").textContent = missing ?? "このコンピューターの中で動く言語モデルで整えます";
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
  cancel.textContent = "取り消し";
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
  el("mine-heading").textContent = `自分で登録した言葉（${mine.length}）`;
  el("user-words").replaceChildren(...mine.map(wordRow));
  el("user-empty").hidden = mine.length > 0;
  const off = builtin.filter((e) => !e.enabled).length;
  el("builtin-heading").textContent = `標準の言葉（${builtin.length}${off > 0 ? `、うち ${off} 語はオフ` : ""}）`;
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

// ---------------------------------------------------------------- advanced

function fillModelSelect(select: HTMLSelectElement, kind: ModelKind, chosen: string | null): void {
  const option = (value: string, text: string) => {
    const o = document.createElement("option");
    o.value = value;
    o.textContent = text;
    return o;
  };
  const files = status.models.filter((m) => m.kind === kind);
  const options = [option("", "自動（空きメモリに合わせて選択）")];
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
  el("light-note").textContent = `軽量モデルはまだダウンロードしていません（合計 ${formatBytes(light.reduce((s, m) => s + m.bytes, 0))}）`;
  el<HTMLSelectElement>("max-minutes").value = String(Math.round(settings.maxRecordingSeconds / 60));
  el("models-dir").textContent = status.modelsDir;
  el("memory").textContent = formatMemory(status.availableMemoryMb);
  el("version").textContent = ctx.version;
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
  [ctx, settings, dictionary] = await Promise.all([api.uiContext(), api.getSettings(), api.getDictionary()]);
  setUpTabs();
  el("builtin-toggle").querySelector(".chevron")!.innerHTML = icons.chevron;

  const picker = mountShortcutPicker(
    el("shortcut-choices"),
    el("shortcut-error"),
    ctx.platform,
    () => settings.shortcut,
    async (shortcut) => {
      await api.saveSettings({ ...settings, shortcut });
      settings = await api.getSettings();
      ctx = await api.uiContext();
      showSaved("shortcut");
    },
  );

  for (const [id, key] of SWITCHES) {
    const input = el<HTMLInputElement>(id);
    input.addEventListener("change", async () => {
      if (!(await save({ [key]: input.checked }, "behavior", "behavior-error"))) input.checked = !input.checked;
      renderGeneral();
      renderBanners();
    });
  }
  const autostart = el<HTMLInputElement>("autostart");
  autostart.checked = await api.getAutostart();
  autostart.addEventListener("change", async () => {
    try {
      await api.setAutostart(autostart.checked);
      el("behavior-error").textContent = "";
    } catch (e) {
      el("behavior-error").textContent = errorText(e);
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
  el("redo-setup").addEventListener("click", () => void api.openSetup());

  // Permissions, tools and models can change outside this window; refresh when it regains focus
  window.addEventListener("focus", () => void refresh());
  await events.onSettingsChanged(async () => {
    settings = await api.getSettings();
    await Promise.all([refresh(), picker.render()]);
  });

  renderDictionary();
  await Promise.all([refresh(), picker.render()]);
}

void main();
