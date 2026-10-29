/** `h:mm:ss` or `m:ss` */
export function clock(secs: number, forceHours = false): string {
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor(s / 60) % 60;
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 || forceHours ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

/** Parses `ss`, `m:ss` or `h:mm:ss`; returns null for empty or invalid input. */
export function parseClock(text: string): number | null {
  const t = text.trim();
  if (!t) return null;
  const parts = t.split(":").map(Number);
  if (parts.some((p) => !Number.isFinite(p) || p < 0) || parts.length > 3) return null;
  return parts.reduce((acc, p) => acc * 60 + p, 0);
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)} MB`;
  return `${(n / 1e3).toFixed(0)} kB`;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function duration(secs: number): string {
  if (secs < 60) return `${Math.round(secs)} s`;
  const m = Math.round(secs / 60);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** Joins words like the backend does: no space before closing punctuation. */
export function joinWords(words: string[]): string {
  let out = "";
  for (const w of words) {
    if (out && !/^[,.!?:;)…]/.test(w)) out += " ";
    out += w;
  }
  return out;
}

const SPEAKER_COLORS = [
  "#2f7f86",
  "#c0703a",
  "#6d5bb5",
  "#3f8f4f",
  "#b5487a",
  "#8a7a2c",
  "#3a6fb5",
  "#9a4a3a",
];

export function speakerColor(index: number): string {
  return SPEAKER_COLORS[((index % SPEAKER_COLORS.length) + SPEAKER_COLORS.length) % SPEAKER_COLORS.length];
}

export const LANGUAGES: [string, string][] = [
  ["de", "Deutsch"],
  ["en", "English"],
  ["fr", "Français"],
  ["es", "Español"],
  ["it", "Italiano"],
  ["nl", "Nederlands"],
  ["pt", "Português"],
  ["pl", "Polski"],
  ["cs", "Čeština"],
  ["da", "Dansk"],
  ["sv", "Svenska"],
  ["no", "Norsk"],
  ["fi", "Suomi"],
  ["el", "Ελληνικά"],
  ["tr", "Türkçe"],
  ["ru", "Русский"],
  ["uk", "Українська"],
  ["ar", "العربية"],
  ["fa", "فارسی"],
  ["he", "עברית"],
  ["hi", "हिन्दी"],
  ["zh", "中文"],
  ["ja", "日本語"],
  ["ko", "한국어"],
];
