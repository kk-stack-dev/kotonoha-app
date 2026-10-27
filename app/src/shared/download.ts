// One progress bar for a whole set of downloads, with a cancel button. Used by setup and settings.

import { api, events, isCancelled } from "./api";
import { formatBytes, purposeLabel } from "./format";
import type { DownloadProgress, DownloadSet } from "./types";

export interface DownloadView {
  /** Wraps the bar, label and cancel button; shown while downloading. */
  box: HTMLElement;
  bar: HTMLElement;
  label: HTMLElement;
  cancel: HTMLButtonElement;
}

export function progressText(p: DownloadProgress): string {
  const percent = Math.floor((p.done / Math.max(p.total, 1)) * 100);
  const position = p.count > 1 ? `${p.count} 個中 ${p.index} 個目（${purposeLabel[p.purpose]}）` : purposeLabel[p.purpose];
  return `${position}  ${formatBytes(p.done)} / ${formatBytes(p.total)}（${percent}%）`;
}

/** Resolves true when everything arrived, false when cancelled; rejects with a user-facing message. */
export async function runDownload(set: DownloadSet, view: DownloadView): Promise<boolean> {
  view.box.hidden = false;
  view.bar.style.width = "0%";
  view.label.textContent = "準備しています";
  view.cancel.disabled = false;
  const onCancel = () => {
    view.cancel.disabled = true;
    view.label.textContent = "中止しています";
    void api.cancelDownload();
  };
  view.cancel.addEventListener("click", onCancel);
  const unlisten = await events.onDownload((p) => {
    view.bar.style.width = `${Math.floor((p.done / Math.max(p.total, 1)) * 100)}%`;
    view.bar.parentElement?.setAttribute("aria-valuenow", String(Math.floor((p.done / Math.max(p.total, 1)) * 100)));
    if (!view.cancel.disabled) view.label.textContent = progressText(p);
  });
  try {
    await api.startDownload(set);
    return true;
  } catch (e) {
    if (isCancelled(e)) return false;
    throw e;
  } finally {
    unlisten();
    view.cancel.removeEventListener("click", onCancel);
    view.box.hidden = true;
  }
}
