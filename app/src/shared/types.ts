// Mirrors of the Rust types serialised by the app (kotonoha-core). Keep in sync with serde attributes.

export type Platform = "macOS" | "windows" | "linux";
export type Modifier = "ctrl" | "alt" | "shift" | "super";

export interface Shortcut {
  modifiers: Modifier[];
  /** W3C KeyboardEvent.code, e.g. "Space", "KeyA", "F2". */
  key: string;
}

export interface Settings {
  shortcut: Shortcut;
  maxRecordingSeconds: number;
  language: string;
  removeFillers: boolean;
  lineBreaks: boolean;
  llmRewrite: boolean;
  autoInsert: boolean;
  keepOnClipboard: boolean;
  models: { directory: string | null; whisper: string | null; llm: string | null };
  tools: { whisper: string | null; llama: string | null };
}

export interface DictionaryEntry {
  word: string;
  variants: string[];
  promptHint: boolean;
  source: "builtin" | "user";
  enabled: boolean;
}

export interface Dictionary {
  entries: DictionaryEntry[];
}

export type CopyReason = "autoInsertOff" | "permissionMissing" | "secureField" | "pasteFailed";

export type Failure =
  | { code: "modelMissing" }
  | { code: "micPermission" }
  | { code: "micUnavailable"; detail: string }
  | { code: "micStartTimedOut" }
  | { code: "micLost"; detail: string }
  | { code: "noAudio"; detail: string }
  | { code: "tooShort" }
  | { code: "nothingRecognized" }
  | { code: "transcription"; detail: string };

export type Outcome =
  | { kind: "inserted" }
  | { kind: "copied"; reason: CopyReason }
  | { kind: "failed"; failure: Failure };

export type Phase =
  | { phase: "idle" }
  | { phase: "starting" }
  | { phase: "recording"; elapsedSecs: number; limitSecs: number }
  | { phase: "transcribing"; remainingChunks: number }
  | { phase: "rewriting" }
  | { phase: "inserting" }
  | { phase: "finished"; outcome: Outcome };

export interface UiContext {
  platform: Platform;
  shortcutLabel: string;
  pasteLabel: string;
  version: string;
}

export type MemoryMode = "normal" | "low" | "critical";

export type ModelKind = "whisper" | "llm";
export type ModelTier = "standard" | "light";
export type Tool = "whisper" | "llama";

export interface ModelFile {
  name: string;
  path: string;
  kind: ModelKind;
  bytes: number;
}

export interface CatalogEntry {
  file: string;
  bytes: number;
  kind: ModelKind;
  tier: ModelTier;
  present: boolean;
}

export interface PlannedDownload {
  file: string;
  bytes: number;
  destination: "tools" | "models";
  /** What the file is for: speech recognition or text clean-up. */
  purpose: ModelKind;
}

export interface SetupPlan {
  downloads: PlannedDownload[];
  /** Missing tools with no download for this machine; the user installs these. */
  manual: Tool[];
}

export interface SetupStatus {
  whisperTool: string | null;
  llamaTool: string | null;
  modelsDir: string;
  models: ModelFile[];
  autoWhisper: string | null;
  autoLlm: string | null;
  catalog: CatalogEntry[];
  plan: SetupPlan;
  planBytes: number;
  /** Dictation works: a speech recognition tool and model are in place. */
  ready: boolean;
  inputPermission: boolean;
  microphonePermission: "granted" | "denied" | "notDetermined";
  memoryMode: MemoryMode;
  availableMemoryMb: number;
}

/** Progress over a whole set of downloads. `index` is 1-based. */
export interface DownloadProgress {
  index: number;
  count: number;
  done: number;
  total: number;
  purpose: ModelKind;
}

export type DownloadSet = "setup" | "lightModels";
