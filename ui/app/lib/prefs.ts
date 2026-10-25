// How dates, times and colours are shown. Kept on the server; the page
// holds a copy and a copy in this browser so the first paint is right.

import { setLanguage } from "./i18n";

export interface Prefs {
  language: string;
  date_format: "locale" | "iso" | "dmy" | "mdy";
  time_format: "locale" | "12h" | "24h";
  theme: "auto" | "light" | "dark";
}

export const DEFAULT_PREFS: Prefs = {
  language: "en",
  date_format: "locale",
  time_format: "locale",
  theme: "auto",
};

let current: Prefs = DEFAULT_PREFS;

export const prefs = () => current;

const STORE = "omp-prefs";


/** The copy from the last visit, if this browser allows storage. */
export function remembered(): Prefs {
  try {
    const raw = localStorage.getItem(STORE);
    if (raw) return { ...DEFAULT_PREFS, ...JSON.parse(raw) };
  } catch {
    // Private windows and blocked storage: use the defaults.
  }
  return DEFAULT_PREFS;
}

export function applyPrefs(next: Prefs) {
  current = next;
  setLanguage(next.language);
  if (typeof document !== "undefined") {
    const root = document.documentElement;
    if (next.theme === "auto") delete root.dataset.theme;
    else root.dataset.theme = next.theme;
    root.lang = next.language;
  }
  try {
    localStorage.setItem(STORE, JSON.stringify(next));
  } catch {
    // Not remembering is fine.
  }
}

const pad = (n: number) => String(n).padStart(2, "0");

export function formatDateTime(d: Date, withTime = true): string {
  const p = current;
  let date: string;
  switch (p.date_format) {
    case "iso":
      date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
      break;
    case "dmy":
      date = `${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()}`;
      break;
    case "mdy":
      date = `${pad(d.getMonth() + 1)}/${pad(d.getDate())}/${d.getFullYear()}`;
      break;
    default:
      date = d.toLocaleDateString();
  }
  if (!withTime) return date;
  let time: string;
  switch (p.time_format) {
    case "24h":
      time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
      break;
    case "12h":
      time = `${d.getHours() % 12 || 12}:${pad(d.getMinutes())} ${d.getHours() < 12 ? "AM" : "PM"}`;
      break;
    default:
      time = d.toLocaleTimeString();
  }
  return `${date} ${time}`;
}
