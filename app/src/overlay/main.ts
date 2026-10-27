// The recording overlay: shows the session state near the bottom of the screen.

import "../shared/base.css";
import "./overlay.css";

import { api, events } from "../shared/api";
import { icons } from "../shared/icons";
import { formatClock, phaseMessage, type Tone } from "../shared/messages";
import type { Phase, UiContext } from "../shared/types";

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const pill = el<HTMLDivElement>("pill");
const icon = el<HTMLSpanElement>("icon");
const title = el<HTMLDivElement>("title");
const detail = el<HTMLDivElement>("detail");
const clock = el<HTMLSpanElement>("clock");
const meter = el<HTMLSpanElement>("meter");
const meterFill = el<HTMLSpanElement>("meter-fill");

const toneIcon: Record<Tone, string> = {
  neutral: icons.mic,
  recording: icons.mic,
  busy: icons.spinner,
  success: icons.check,
  info: icons.clipboard,
  warning: icons.warning,
};

let ctx: UiContext;

function render(phase: Phase): void {
  if (phase.phase === "idle") {
    pill.hidden = true;
    return;
  }
  const message = phaseMessage(phase, ctx);
  pill.className = `pill tone-${message.tone}`;
  pill.hidden = false;
  icon.innerHTML = toneIcon[message.tone];
  title.textContent = message.title;
  detail.textContent = message.detail ?? "";

  const recording = phase.phase === "recording";
  clock.hidden = !recording;
  meter.hidden = !recording;
  if (recording) {
    clock.textContent = formatClock(phase.elapsedSecs);
  } else {
    meterFill.style.width = "0";
  }
}

function showLevel(level: number): void {
  // Speech peaks around 0.05-0.3 RMS; a square-root curve keeps quiet speech visible
  const ratio = Math.min(1, Math.sqrt(level / 0.25));
  meterFill.style.width = `${Math.round(ratio * 100)}%`;
}

async function main(): Promise<void> {
  ctx = await api.uiContext();
  await events.onSettingsChanged(async () => {
    ctx = await api.uiContext();
  });
  await events.onPhase(render);
  await events.onLevel(showLevel);
  render(await api.currentPhase());
}

void main();
