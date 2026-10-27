// Typed wrappers around the app's commands and events.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Dictionary,
  DownloadProgress,
  DownloadSet,
  Phase,
  Settings,
  SetupStatus,
  Shortcut,
  UiContext,
} from "./types";

type MicPermission = SetupStatus["microphonePermission"];

export const api = {
  uiContext: () => invoke<UiContext>("ui_context"),
  currentPhase: () => invoke<Phase>("current_phase"),
  toggleRecording: () => invoke<void>("toggle_recording"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  shortcutLabel: (shortcut: Shortcut) => invoke<string>("shortcut_label", { shortcut }),
  suspendShortcut: () => invoke<void>("suspend_shortcut"),
  resumeShortcut: () => invoke<void>("resume_shortcut"),
  getDictionary: () => invoke<Dictionary>("get_dictionary"),
  addWord: (word: string, variants: string) => invoke<Dictionary>("add_word", { word, variants }),
  removeWord: (word: string) => invoke<Dictionary>("remove_word", { word }),
  editWord: (word: string, newWord: string, variants: string) =>
    invoke<Dictionary>("edit_word", { word, newWord, variants }),
  setWordEnabled: (word: string, enabled: boolean) => invoke<Dictionary>("set_word_enabled", { word, enabled }),
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  startDownload: (set: DownloadSet) => invoke<void>("start_download", { set }),
  cancelDownload: () => invoke<void>("cancel_download"),
  requestMicrophone: () => invoke<MicPermission>("request_microphone"),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  openInputPermissionSettings: () => invoke<void>("open_input_permission_settings"),
  openModelsFolder: () => invoke<void>("open_models_folder"),
  openSetup: () => invoke<void>("open_setup"),
  finishSetup: () => invoke<void>("finish_setup"),
  openLogs: () => invoke<void>("open_logs"),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
};

export const events = {
  onPhase: (handler: (phase: Phase) => void): Promise<UnlistenFn> =>
    listen<Phase>("session-phase", (e) => handler(e.payload)),
  onLevel: (handler: (level: number) => void): Promise<UnlistenFn> =>
    listen<number>("session-level", (e) => handler(e.payload)),
  onSettingsChanged: (handler: () => void): Promise<UnlistenFn> => listen("settings-changed", () => handler()),
  onDownload: (handler: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
    listen<DownloadProgress>("download-progress", (e) => handler(e.payload)),
};

/** Command errors arrive as plain strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

/** A download stopped with the cancel button; not an error to show. */
export function isCancelled(e: unknown): boolean {
  return errorText(e) === "cancelled";
}
