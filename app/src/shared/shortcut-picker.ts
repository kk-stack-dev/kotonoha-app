// Choosing the recording shortcut: a few presets for this OS, or any key combination pressed while
// capturing. Used by setup and settings.

import { api, errorText } from "./api";
import type { Modifier, Platform, Shortcut } from "./types";

const PRESETS: Record<Platform, Shortcut[]> = {
  macOS: [
    { modifiers: ["alt"], key: "Space" },
    { modifiers: ["ctrl", "alt"], key: "Space" },
    { modifiers: [], key: "F5" },
  ],
  windows: [
    { modifiers: ["ctrl", "alt"], key: "Space" },
    { modifiers: ["ctrl", "shift"], key: "Space" },
    { modifiers: [], key: "F9" },
  ],
  linux: [
    { modifiers: ["ctrl", "alt"], key: "Space" },
    { modifiers: [], key: "F9" },
  ],
};

// Hold-to-talk is a separate key from the tap shortcut, so its presets differ from the ones above
const HOLD_PRESETS: Record<Platform, Shortcut[]> = {
  macOS: [
    { modifiers: ["alt", "shift"], key: "Space" },
    { modifiers: [], key: "F6" },
  ],
  windows: [
    { modifiers: ["ctrl", "alt", "shift"], key: "Space" },
    { modifiers: [], key: "F10" },
  ],
  linux: [
    { modifiers: ["ctrl", "alt", "shift"], key: "Space" },
    { modifiers: [], key: "F10" },
  ],
};

const MODIFIER_CODES = /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/;

const same = (a: Shortcut, b: Shortcut) =>
  a.key === b.key && a.modifiers.length === b.modifiers.length && a.modifiers.every((m) => b.modifiers.includes(m));

export interface PickerOptions {
  /** Hold-to-talk: shows its own presets and a "not used" choice (saved as null). */
  optional?: boolean;
}

export interface ShortcutPicker {
  render(): Promise<void>;
}

/**
 * Fills `container` with buttons. `save` stores the shortcut (and throws a user-facing message when the
 * OS refuses it); `current` returns the saved one.
 */
export function mountShortcutPicker(
  container: HTMLElement,
  error: HTMLElement,
  platform: Platform,
  current: () => Shortcut | null,
  save: (shortcut: Shortcut | null) => Promise<void>,
  options: PickerOptions = {},
): ShortcutPicker {
  const optional = options.optional ?? false;
  let capturing = false;
  const captureButton = document.createElement("button");
  captureButton.type = "button";
  captureButton.className = "choice other";

  async function apply(shortcut: Shortcut | null): Promise<void> {
    try {
      if (shortcut) await api.shortcutLabel(shortcut);
      await save(shortcut);
      error.textContent = "";
    } catch (e) {
      error.textContent = errorText(e);
    }
  }

  async function startCapture(): Promise<void> {
    capturing = true;
    error.textContent = "";
    captureButton.classList.add("capturing");
    captureButton.textContent = "キーを入力してください（Esc でキャンセル）";
    // Free the current shortcut so pressing it here does not start a recording
    await api.suspendShortcut();
  }

  async function endCapture(): Promise<void> {
    if (!capturing) return;
    capturing = false;
    captureButton.classList.remove("capturing");
    await api.resumeShortcut().catch((e) => (error.textContent = errorText(e)));
    await render();
  }

  window.addEventListener(
    "keydown",
    (event) => {
      if (!capturing) return;
      event.preventDefault();
      const modifiers: Modifier[] = [];
      if (event.ctrlKey) modifiers.push("ctrl");
      if (event.altKey) modifiers.push("alt");
      if (event.shiftKey) modifiers.push("shift");
      if (event.metaKey) modifiers.push("super");
      if (event.code === "Escape" && modifiers.length === 0) {
        void endCapture();
        return;
      }
      // Wait for the main key; KeyboardEvent.code names the physical key, independent of layout and IME
      if (MODIFIER_CODES.test(event.code)) return;
      void apply({ modifiers, key: event.code }).then(endCapture);
    },
    true,
  );
  // Leaving the window mid-capture would leave the shortcut switched off
  window.addEventListener("blur", () => void endCapture());
  captureButton.addEventListener("click", () => void (capturing ? endCapture() : startCapture()));

  async function render(): Promise<void> {
    const saved = current();
    const presets = (optional ? HOLD_PRESETS : PRESETS)[platform];
    const off = document.createElement("button");
    off.type = "button";
    off.className = "choice";
    off.textContent = "使わない";
    off.setAttribute("aria-pressed", String(saved === null));
    off.addEventListener("click", () => void apply(null).then(render));
    const buttons = await Promise.all(
      presets.map(async (preset) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "choice";
        button.textContent = await api.shortcutLabel(preset);
        button.setAttribute("aria-pressed", String(saved !== null && same(preset, saved)));
        button.addEventListener("click", () => void apply(preset).then(render));
        return button;
      }),
    );
    const custom = saved !== null && !presets.some((p) => same(p, saved));
    captureButton.textContent = custom ? await api.shortcutLabel(saved) : "キーを入力して変更...";
    captureButton.setAttribute("aria-pressed", String(custom));
    container.replaceChildren(...(optional ? [off] : []), ...buttons, captureButton);
  }

  return { render };
}
