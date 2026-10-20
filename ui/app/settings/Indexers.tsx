"use client";

import { usePluginImplementations } from "./PluginFields";
import { ProviderManager, type Implementation } from "./ProviderEditor";

function Fields({ settings, onChange }: { settings: Record<string, unknown>; onChange: (s: Record<string, unknown>) => void }) {
  const text = (k: string) => String(settings[k] ?? "");
  const set = (k: string, v: unknown) => onChange({ ...settings, [k]: v });
  const list = (k: string) => (Array.isArray(settings[k]) ? (settings[k] as number[]).join(", ") : "");
  const numbers = (value: string) =>
    value
      .split(",")
      .map((c) => parseInt(c.trim(), 10))
      .filter((n) => Number.isFinite(n));
  return (
    <div className="form-grid">
      <label className="field">
        <span>URL</span>
        <input type="text" placeholder="https://indexer.example" value={text("base_url")} onChange={(e) => set("base_url", e.target.value.trim())} />
      </label>
      <label className="field">
        <span>API key</span>
        <input type="password" autoComplete="off" value={text("api_key")} onChange={(e) => set("api_key", e.target.value.trim())} />
      </label>
      <label className="field">
        <span>Movie categories (comma separated)</span>
        <input type="text" defaultValue={list("categories")} onBlur={(e) => set("categories", numbers(e.target.value))} />
      </label>
      <label className="field">
        <span>TV categories (comma separated)</span>
        <input type="text" defaultValue={list("tv_categories")} placeholder="5030, 5040" onBlur={(e) => set("tv_categories", numbers(e.target.value))} />
      </label>
      <label className="field">
        <span>Anime categories (empty skips anime)</span>
        <input type="text" defaultValue={list("anime_categories")} placeholder="5070" onBlur={(e) => set("anime_categories", numbers(e.target.value))} />
      </label>
      <label className="check">
        <input type="checkbox" checked={settings.anime_standard_format_search === true} onChange={(e) => set("anime_standard_format_search", e.target.checked)} /> Also search anime by season and episode number
      </label>
    </div>
  );
}

const defaults = { base_url: "", api_key: "", categories: [2000, 2010, 2020, 2030, 2040, 2045, 2050, 2060], tv_categories: [5030, 5040], anime_categories: [] as number[], anime_standard_format_search: false };
const describe = (s: Record<string, unknown>) => String(s.base_url ?? "");

const IMPLEMENTATIONS: Implementation[] = [
  { id: "newznab", label: "Newznab (usenet)", defaults, Fields, describe },
  { id: "torznab", label: "Torznab (torrent)", defaults: { ...defaults, minimum_seeders: 1 }, Fields, describe },
];

export function Indexers() {
  const fromPlugins = usePluginImplementations("indexer");
  return (
    <ProviderManager
      path="indexer"
      noun="indexer"
      implementations={[...IMPLEMENTATIONS, ...fromPlugins]}
      searchSwitches
      help="Indexers are where releases are found. Any Newznab or Torznab compatible site works, and plugins can add others. Use the Test button to check the settings."
    />
  );
}
