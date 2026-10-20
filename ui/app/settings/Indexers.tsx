"use client";

import { usePluginImplementations } from "./PluginFields";
import { ProviderManager, type Implementation } from "./ProviderEditor";

function Fields({ settings, onChange }: { settings: Record<string, unknown>; onChange: (s: Record<string, unknown>) => void }) {
  const text = (k: string) => String(settings[k] ?? "");
  const set = (k: string, v: unknown) => onChange({ ...settings, [k]: v });
  const categories = Array.isArray(settings.categories) ? (settings.categories as number[]).join(", ") : "";
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
        <span>Categories (comma separated)</span>
        <input
          type="text"
          defaultValue={categories}
          onBlur={(e) =>
            set(
              "categories",
              e.target.value
                .split(",")
                .map((c) => parseInt(c.trim(), 10))
                .filter((n) => Number.isFinite(n)),
            )
          }
        />
      </label>
    </div>
  );
}

const defaults = { base_url: "", api_key: "", categories: [2000, 2010, 2020, 2030, 2040, 2045, 2050, 2060] };
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
