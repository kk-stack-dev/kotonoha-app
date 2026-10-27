// Fails when UI or Rust sources contain characters that render differently (or not at all) across
// operating systems and fonts: emoji, Mac-only key symbols, private-use and other machine-dependent
// characters (circled numbers, unit squares, ...). Key names must be words ("Option + Space").

import { readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";

const root = new URL("../..", import.meta.url).pathname;
const scanDirs = ["app/src", "app/settings.html", "app/setup.html", "app/overlay.html", "app/src-tauri/src", "crates"];
const extensions = new Set([".ts", ".css", ".html", ".rs", ".json"]);

const forbidden = [
  [0x1f000, 0x1faff, "emoji"],
  [0x2600, 0x27bf, "symbol / dingbat"],
  [0xfe0f, 0xfe0f, "emoji variation selector"],
  [0x2300, 0x23ff, "technical symbol (Mac key glyphs)"],
  [0x21e7, 0x21e7, "Shift glyph"],
  [0xe000, 0xf8ff, "private use"],
  [0x2460, 0x24ff, "enclosed alphanumeric"],
  [0x3200, 0x33ff, "enclosed CJK / unit square"],
];

function* files(path) {
  const stat = statSync(path);
  if (stat.isFile()) {
    if (extensions.has(extname(path))) yield path;
    return;
  }
  for (const name of readdirSync(path)) {
    if (name === "node_modules" || name === "target" || name === "gen") continue;
    yield* files(join(path, name));
  }
}

let problems = 0;
for (const dir of scanDirs) {
  for (const file of files(join(root, dir))) {
    readFileSync(file, "utf8")
      .split("\n")
      .forEach((line, i) => {
        for (const ch of line) {
          const cp = ch.codePointAt(0);
          const hit = forbidden.find(([lo, hi]) => cp >= lo && cp <= hi);
          if (hit) {
            problems++;
            console.error(`${relative(root, file)}:${i + 1}: U+${cp.toString(16).toUpperCase()} (${hit[2]})`);
          }
        }
      });
  }
}
if (problems > 0) {
  console.error(`\n${problems} environment-dependent character(s) found.`);
  process.exit(1);
}
console.log("No environment-dependent characters found.");
