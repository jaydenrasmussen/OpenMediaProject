"use client";

import { useMemo, useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Catalogue, Provider, SiteDetail, SiteField, SiteSummary, UsenetPreset } from "../lib/types";
import { usePluginImplementations } from "./PluginFields";
import { Editor, type Implementation } from "./ProviderEditor";

// ---- the form for a usenet (Newznab) or Torznab indexer ---------------------------

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
        <span>API path</span>
        <input type="text" value={text("api_path") || "/api"} onChange={(e) => set("api_path", e.target.value.trim())} />
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
      <label className="field">
        <span>Season search: require the pack once the season is older than (days, 0 off)</span>
        <input type="number" min={0} value={Number(settings.season_search_maximum_single_episode_age ?? 0)} onChange={(e) => set("season_search_maximum_single_episode_age", Number(e.target.value))} />
      </label>
      {"minimum_seeders" in settings && <Seeding settings={settings} set={set} />}
      <label className="check">
        <input type="checkbox" checked={settings.anime_standard_format_search === true} onChange={(e) => set("anime_standard_format_search", e.target.checked)} /> Also search anime by season and episode number
      </label>
    </div>
  );
}

function Seeding({ settings, set }: { settings: Record<string, unknown>; set: (k: string, v: unknown) => void }) {
  return (
    <>
      <label className="field">
        <span>Minimum seeders</span>
        <input type="number" min={0} value={Number(settings.minimum_seeders ?? 1)} onChange={(e) => set("minimum_seeders", Number(e.target.value))} />
      </label>
      <label className="field">
        <span>Seed ratio (empty uses the download client’s)</span>
        <input type="number" min={0} step={0.1} value={settings.seed_ratio == null ? "" : Number(settings.seed_ratio)} onChange={(e) => set("seed_ratio", e.target.value === "" ? null : Number(e.target.value))} />
      </label>
      <label className="field">
        <span>Seed time in minutes (empty uses the download client’s)</span>
        <input type="number" min={0} value={settings.seed_time_minutes == null ? "" : Number(settings.seed_time_minutes)} onChange={(e) => set("seed_time_minutes", e.target.value === "" ? null : Number(e.target.value))} />
      </label>
    </>
  );
}

// ---- the form for a torrent site, drawn from its definition -------------------------

const plain = (html: string) => html.replace(/<br\s*\/?>/gi, " ").replace(/<[^>]+>/g, "").trim();

function SiteFields({ settings, onChange }: { settings: Record<string, unknown>; onChange: (s: Record<string, unknown>) => void }) {
  const id = String(settings.definition ?? "");
  const site = useLoad(() => get<SiteDetail>(`/indexer/site/${id}`), [id]);
  const config = (settings.config ?? {}) as Record<string, unknown>;
  const set = (k: string, v: unknown) => onChange({ ...settings, [k]: v });
  const setConfig = (k: string, v: unknown) => onChange({ ...settings, config: { ...config, [k]: v } });
  if (site.error) return <p className="error">{site.error}</p>;
  if (!site.data) return <p className="muted">Loading…</p>;
  const d = site.data;
  const value = (f: SiteField) => (f.name in config ? config[f.name] : (f.default ?? (f.kind === "select" ? (f.options[0]?.[0] ?? "") : "")));
  return (
    <div className="stack">
      {d.description && <p className="muted small">{d.description}</p>}
      {d.cloudflare && <p className="warn small">This site is often behind Cloudflare’s check, which this app can’t pass on its own. If searching fails with that message, try another site.</p>}
      <div className="form-grid">
        {d.links.length > 1 || settings.base_url ? (
          <label className="field">
            <span>Address</span>
            <input list={`links-${d.id}`} type="text" placeholder={d.links[0]} value={String(settings.base_url ?? "")} onChange={(e) => set("base_url", e.target.value.trim())} />
            <datalist id={`links-${d.id}`}>
              {d.links.map((l) => (
                <option key={l} value={l} />
              ))}
            </datalist>
          </label>
        ) : null}
        {d.fields.map((f) =>
          f.kind === "info" ? null : f.kind === "checkbox" ? (
            <label key={f.name} className="check">
              <input type="checkbox" checked={String(value(f)).toLowerCase() === "true"} onChange={(e) => setConfig(f.name, e.target.checked)} /> {f.label}
            </label>
          ) : f.kind === "select" ? (
            <label key={f.name} className="field">
              <span>{f.label}</span>
              <select value={String(value(f))} onChange={(e) => setConfig(f.name, e.target.value)}>
                {f.options.map(([k, label]) => (
                  <option key={k} value={k}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <label key={f.name} className="field">
              <span>{f.label}</span>
              <input
                type={f.kind === "password" ? "password" : "text"}
                autoComplete="off"
                value={String(value(f))}
                onChange={(e) => setConfig(f.name, e.target.value)}
              />
            </label>
          ),
        )}
        <Seeding settings={settings} set={set} />
      </div>
      {d.fields
        .filter((f) => f.kind === "info" && f.default && plain(f.default))
        .map((f) => (
          <p key={f.name} className="muted small">
            {plain(f.default ?? "")}
          </p>
        ))}
    </div>
  );
}

const usenetDefaults = { base_url: "", api_path: "/api", api_key: "", categories: [2000, 2010, 2020, 2030, 2040, 2045, 2050, 2060], tv_categories: [5030, 5040], anime_categories: [] as number[], anime_standard_format_search: false };
const describe = (s: Record<string, unknown>) => String(s.base_url ?? "");

const IMPLEMENTATIONS: Implementation[] = [
  { id: "newznab", label: "Usenet indexer (Newznab)", defaults: usenetDefaults, Fields, describe },
  { id: "torznab", label: "Torrent indexer (Torznab)", defaults: { ...usenetDefaults, minimum_seeders: 1, seed_ratio: null, seed_time_minutes: null }, Fields, describe },
  {
    id: "cardigann",
    label: "Torrent site",
    defaults: { definition: "", config: {}, base_url: "", minimum_seeders: 1, seed_ratio: null, seed_time_minutes: null },
    Fields: SiteFields,
    describe: (s) => String(s.definition ?? ""),
  },
];

// ---- the list of sites to choose from ------------------------------------------------

const PRIVACY: [string, string][] = [
  ["", "All"],
  ["public", "Public"],
  ["semi-private", "Semi-private"],
  ["private", "Private"],
];

const languageName = (code: string) => {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" }).of(code.split("-")[0]) ?? code;
  } catch {
    return code;
  }
};

const PAGE = 60;

function TorrentSites({ sites, onPick }: { sites: SiteSummary[]; onPick: (s: SiteSummary) => void }) {
  const [text, setText] = useState("");
  const [privacy, setPrivacy] = useState("");
  const [language, setLanguage] = useState("");
  const [media, setMedia] = useState<string[]>([]);
  const [noExtras, setNoExtras] = useState(false);
  const [shown, setShown] = useState(PAGE);

  const languages = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of sites) counts.set(s.language, (counts.get(s.language) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  }, [sites]);

  // Sites anyone can use come first.
  const ranked = useMemo(() => {
    const order: Record<string, number> = { public: 0, "semi-private": 1, private: 2 };
    return [...sites].sort((a, b) => (order[a.privacy] ?? 3) - (order[b.privacy] ?? 3) || a.name.localeCompare(b.name));
  }, [sites]);

  const found = useMemo(() => {
    const words = text.toLowerCase().split(/\s+/).filter(Boolean);
    return ranked.filter((s) => {
      if (privacy && s.privacy !== privacy) return false;
      if (language && s.language !== language) return false;
      if (media.includes("Movies") && !s.movies) return false;
      if (media.includes("TV") && !s.tv) return false;
      if (media.includes("Anime") && !s.categories.includes("Anime")) return false;
      if (noExtras && (s.cloudflare || s.needs_account)) return false;
      const hay = `${s.name} ${s.description} ${s.links.join(" ")}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  }, [ranked, text, privacy, language, media, noExtras]);

  const reset = () => setShown(PAGE);
  const toggle = (m: string) => {
    setMedia(media.includes(m) ? media.filter((x) => x !== m) : [...media, m]);
    reset();
  };

  return (
    <div className="stack">
      <div className="row">
        <input type="search" className="grow" placeholder={`Search ${sites.length} torrent sites`} value={text} onChange={(e) => { setText(e.target.value); reset(); }} />
        <select aria-label="Language" value={language} onChange={(e) => { setLanguage(e.target.value); reset(); }}>
          <option value="">Any language</option>
          {languages.map(([code, n]) => (
            <option key={code} value={code}>
              {languageName(code)} ({code.split("-")[1] ?? code}) · {n}
            </option>
          ))}
        </select>
      </div>
      <div className="row">
        <div className="segmented" role="group" aria-label="Who can join">
          {PRIVACY.map(([k, label]) => (
            <button key={k} className={privacy === k ? "active" : ""} onClick={() => { setPrivacy(k); reset(); }}>
              {label}
            </button>
          ))}
        </div>
        <div className="segmented" role="group" aria-label="Media">
          {["Movies", "TV", "Anime"].map((m) => (
            <button key={m} className={media.includes(m) ? "active" : ""} aria-pressed={media.includes(m)} onClick={() => toggle(m)}>
              {m}
            </button>
          ))}
        </div>
        <label className="check">
          <input type="checkbox" checked={noExtras} onChange={(e) => { setNoExtras(e.target.checked); reset(); }} /> Works without an account or Cloudflare
        </label>
      </div>
      <p className="muted small">
        {found.length} of {sites.length} sites. Public sites need nothing; semi-private ones work without an account but better with one; private ones need your account.
      </p>
      {found.slice(0, shown).map((s) => (
        <div key={s.id} className="site-row">
          <div className="site-main">
            <div>
              <strong>{s.name}</strong> <span className={`badge ${s.privacy === "public" ? "good" : s.privacy === "private" ? "warn" : ""}`}>{s.privacy}</span>{" "}
              <span className="badge">{languageName(s.language)}</span>
              {s.cloudflare && <span className="badge warn" title="Often needs a browser to pass Cloudflare's check">Cloudflare</span>}
            </div>
            <div className="muted small">{s.categories.join(" · ")}</div>
            {s.description && <div className="muted small clamp">{s.description}</div>}
          </div>
          <button onClick={() => onPick(s)}>Add</button>
        </div>
      ))}
      {found.length === 0 && <p className="muted">No site matches. Try fewer filters.</p>}
      {found.length > shown && <button onClick={() => setShown(shown + PAGE)}>Show more</button>}
    </div>
  );
}

function UsenetIndexers({ presets, onPick, onOther }: { presets: UsenetPreset[]; onPick: (p: UsenetPreset) => void; onOther: (kind: "newznab" | "torznab") => void }) {
  const [text, setText] = useState("");
  const found = presets.filter((p) => `${p.name} ${p.base_url}`.toLowerCase().includes(text.toLowerCase()));
  return (
    <div className="stack">
      <input type="search" style={{ width: "100%" }} placeholder={`Search ${presets.length} usenet indexers`} value={text} onChange={(e) => setText(e.target.value)} />
      <p className="muted small">You need an account (and an API key) with the indexer. Find the key in the indexer’s website under your profile.</p>
      {found.map((p) => (
        <div key={p.name} className="site-row">
          <div className="site-main">
            <strong>{p.name}</strong>
            <div className="muted small">{p.base_url}</div>
          </div>
          <button onClick={() => onPick(p)}>Add</button>
        </div>
      ))}
      <div className="site-row">
        <div className="site-main">
          <strong>Another Newznab indexer</strong>
          <div className="muted small">Any usenet indexer that speaks Newznab: enter its address and key.</div>
        </div>
        <button onClick={() => onOther("newznab")}>Add</button>
      </div>
      <div className="site-row">
        <div className="site-main">
          <strong>Another torrent indexer (Torznab)</strong>
          <div className="muted small">A Torznab feed from software such as Jackett: enter its address and key.</div>
        </div>
        <button onClick={() => onOther("torznab")}>Add</button>
      </div>
    </div>
  );
}

function FlareSolverr() {
  const notify = useToast();
  const saved = useLoad(() => get<{ url: string }>("/indexer/flaresolverr"));
  const [url, setUrl] = useState<string | null>(null);
  const value = url ?? saved.data?.url ?? "";
  async function save() {
    try {
      await put("/indexer/flaresolverr", { url: value });
      notify(value ? "FlareSolverr is set up." : "FlareSolverr is turned off.", "good");
      saved.reload();
      setUrl(null);
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  return (
    <section className="card stack">
      <h3 style={{ margin: 0 }}>FlareSolverr (optional)</h3>
      <p className="muted small">
        Some sites, such as 1337x and EZTV, sit behind Cloudflare’s browser check, which this app can’t pass alone. If you run <a href="https://github.com/FlareSolverr/FlareSolverr">FlareSolverr</a>, give its address here and the app asks it to pass the check for those sites.
      </p>
      <div className="row">
        <input type="text" className="grow" placeholder="http://flaresolverr:8191" value={value} onChange={(e) => setUrl(e.target.value)} />
        <button onClick={save} disabled={url === null}>
          Save
        </button>
      </div>
    </section>
  );
}

// ---- the page ----------------------------------------------------------------------------

type Draft = Omit<Provider, "id"> & { id?: number };
const blank = (): Omit<Draft, "name" | "implementation" | "settings" | "plugin_id"> => ({
  tags: [],
  enabled: true,
  enable_rss: true,
  enable_automatic_search: true,
  enable_interactive_search: true,
  priority: 25,
});

export function Indexers() {
  const notify = useToast();
  const list = useLoad(() => get<Provider[]>("/indexer"));
  const catalogue = useLoad(() => get<Catalogue>("/indexer/catalogue"));
  const fromPlugins = usePluginImplementations("indexer");
  const implementations = [...IMPLEMENTATIONS, ...fromPlugins];
  const [draft, setDraft] = useState<Draft | null>(null);
  const [adding, setAdding] = useState(false);
  const [tab, setTab] = useState<"torrent" | "usenet">("torrent");

  const siteName = (id: string) => catalogue.data?.torrent.find((s) => s.id === id);

  function startSite(s: SiteSummary) {
    setAdding(false);
    setDraft({
      ...blank(),
      name: s.name,
      implementation: "cardigann",
      plugin_id: null,
      settings: { definition: s.id, config: {}, base_url: "", minimum_seeders: 1, seed_ratio: null, seed_time_minutes: null },
    });
  }
  function startUsenet(p: UsenetPreset) {
    setAdding(false);
    setDraft({ ...blank(), name: p.name, implementation: "newznab", plugin_id: null, settings: { ...structuredClone(usenetDefaults), base_url: p.base_url, api_path: p.api_path } });
  }
  function startOther(kind: "newznab" | "torznab") {
    setAdding(false);
    const impl = IMPLEMENTATIONS.find((i) => i.id === kind)!;
    setDraft({ ...blank(), name: "", implementation: kind, plugin_id: null, settings: structuredClone(impl.defaults) });
  }

  async function remove(p: Provider) {
    if (!confirm(`Delete indexer “${p.name}”?`)) return;
    try {
      await del(`/indexer/${p.id}`);
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  async function toggle(p: Provider) {
    try {
      await put(`/indexer/${p.id}`, { ...p, enabled: !p.enabled });
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  const kindOf = (p: Provider) =>
    p.implementation === "cardigann" ? "Torrent site" : p.implementation === "newznab" ? "Usenet" : p.implementation === "torznab" ? "Torrent (Torznab)" : "Plugin";

  return (
    <div className="stack">
      <p className="muted small">Indexers are where releases are found. Add the torrent sites and usenet indexers you use; the app searches them all.</p>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data?.length === 0 && !draft && !adding && <p className="muted">No indexers yet. Add a torrent site or a usenet indexer.</p>}
      {list.data?.map((p) => {
        const site = p.implementation === "cardigann" ? siteName(String(p.settings.definition ?? "")) : undefined;
        return (
          <div key={p.id} className="card spread">
            <div>
              <strong>{p.name}</strong> <span className="badge">{kindOf(p)}</span>{" "}
              {site && <span className="badge">{site.privacy}</span>} {!p.enabled && <span className="badge warn">Disabled</span>}
              <div className="muted small">{p.implementation === "cardigann" ? (site?.description ?? "") : String(p.settings.base_url ?? "")}</div>
            </div>
            <div className="row">
              <button onClick={() => toggle(p)}>{p.enabled ? "Disable" : "Enable"}</button>
              <button onClick={() => { setAdding(false); setDraft({ ...p, settings: structuredClone(p.settings) }); }}>Edit</button>
              <button className="danger" onClick={() => remove(p)}>
                Delete
              </button>
            </div>
          </div>
        );
      })}

      {draft && (
        <Editor
          key={draft.id ?? draft.name}
          path="indexer"
          noun="indexer"
          draft={draft}
          setDraft={setDraft}
          implementations={implementations}
          searchSwitches
          fixedType
          onClose={() => setDraft(null)}
          onSaved={() => {
            setDraft(null);
            list.reload();
          }}
        />
      )}

      {!draft && !adding && (
        <div>
          <button className="primary" onClick={() => setAdding(true)}>
            Add indexer
          </button>
        </div>
      )}

      {!draft && !adding && <FlareSolverr />}

      {adding && (
        <section className="card stack">
          <div className="spread">
            <h3 style={{ margin: 0 }}>Add an indexer</h3>
            <button onClick={() => setAdding(false)}>Close</button>
          </div>
          <div className="tabs" role="tablist">
            <button role="tab" className={tab === "torrent" ? "active" : ""} onClick={() => setTab("torrent")}>
              Torrent sites
            </button>
            <button role="tab" className={tab === "usenet" ? "active" : ""} onClick={() => setTab("usenet")}>
              Usenet indexers
            </button>
          </div>
          <LoadState loading={catalogue.loading} error={catalogue.error} hasData={!!catalogue.data} />
          {catalogue.data && (tab === "torrent" ? <TorrentSites sites={catalogue.data.torrent} onPick={startSite} /> : <UsenetIndexers presets={catalogue.data.usenet} onPick={startUsenet} onOther={startOther} />)}
        </section>
      )}
    </div>
  );
}
