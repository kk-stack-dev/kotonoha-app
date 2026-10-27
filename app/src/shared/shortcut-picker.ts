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

const MODIFIER_CODES = /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/;

const same = (a: Shortcut, b: Shortcut) =>
  a.key === b.key && a.modifiers.length === b.modifiers.length && a.modifiers.every((m) => b.modifiers.includes(m));

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
  current: () => Shortcut,
  save: (shortcut: Shortcut) => Promise<void>,
): ShortcutPicker {
  let capturing = false;
  const captureButton = document.createElement("button");
  captureButton.type = "button";
  captureButton.className = "choice other";

  async function apply(shortcut: Shortcut): Promise<void> {
    try {
      await api.shortcutLabel(shortcut);
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
    captureButton.textContent = "キーを押してください（Esc でやめる）";
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
    const presets = PRESETS[platform];
    const buttons = await Promise.all(
      presets.map(async (preset) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "choice";
        button.textContent = await api.shortcutLabel(preset);
        button.setAttribute("aria-pressed", String(same(preset, saved)));
        button.addEventListener("click", () => void apply(preset).then(render));
        return button;
      }),
    );
    const custom = !presets.some((p) => same(p, saved));
    captureButton.textContent = custom ? await api.shortcutLabel(saved) : "ほかのキー...";
    captureButton.setAttribute("aria-pressed", String(custom));
    container.replaceChildren(...buttons, captureButton);
  }

  return { render };
}
