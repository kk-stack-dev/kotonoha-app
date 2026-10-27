// Inline SVG icons (stroke = currentColor), so they render the same on every OS and font.

const svg = (body: string) =>
  `<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

export const icons = {
  mic: svg('<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0"/><path d="M12 18v3"/>'),
  check: svg('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
  clipboard: svg('<rect x="6" y="4" width="12" height="17" rx="2"/><path d="M9 4.5h6v-1a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1z"/>'),
  warning: svg('<path d="M12 3.5l9.5 16.5h-19z"/><path d="M12 10v4.5"/><path d="M12 17.5v.01"/>'),
  spinner: svg('<path d="M12 3a9 9 0 1 0 9 9" class="spin"/>'),
  close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
  download: svg('<path d="M12 4v11"/><path d="M7.5 10.5L12 15l4.5-4.5"/><path d="M5 20h14"/>'),
  info: svg('<circle cx="12" cy="12" r="9"/><path d="M12 11v5"/><path d="M12 7.5v.01"/>'),
  chevron: svg('<path d="M9 6l6 6-6 6"/>'),
};
