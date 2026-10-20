"use client";

import { useEffect, useState } from "react";
import { LoadState } from "../components/Page";
import { SchemaForm, schemaDefaults } from "../components/SchemaForm";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Exclusion, PluginList, Provider, QualityProfile, RootFolder, Schema } from "../lib/types";

interface Draft {
  id: number;
  name: string;
  plugin_id: string;
  enabled: boolean;
  quality_profile_id: number;
  root_folder_path: string;
  monitored: boolean;
  search_on_add: boolean;
  minimum_availability: string;
  /** Empty means the list's series are skipped. */
  series_quality_profile_id: number | null;
  series_root_folder_path: string;
  series_monitor: string;
  settings: Record<string, unknown>;
}

type Stored = Partial<Omit<Draft, "id" | "name" | "plugin_id" | "enabled">>;

export function Lists() {
  const notify = useToast();
  const list = useLoad(() => get<Provider[]>("/importlist"));
  const plugins = useLoad(() => get<PluginList>("/plugin"));
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const roots = useLoad(() => get<RootFolder[]>("/rootfolder"));
  const [draft, setDraft] = useState<Draft | null>(null);
  const [syncing, setSyncing] = useState(false);
  const sources = plugins.data?.plugins.filter((p) => p.kind === "list") ?? [];
  const ready = !!profiles.data?.length && !!roots.data?.length;

  function edit(p: Provider) {
    const s = p.settings as Stored;
    setDraft({
      id: p.id,
      name: p.name,
      plugin_id: p.plugin_id ?? "",
      enabled: p.enabled,
      quality_profile_id: s.quality_profile_id ?? profiles.data![0].id,
      root_folder_path: s.root_folder_path ?? roots.data![0].path,
      monitored: s.monitored ?? true,
      search_on_add: s.search_on_add ?? false,
      minimum_availability: s.minimum_availability ?? "announced",
      series_quality_profile_id: s.series_quality_profile_id ?? null,
      series_root_folder_path: s.series_root_folder_path ?? "",
      series_monitor: s.series_monitor ?? "all",
      settings: s.settings ?? {},
    });
  }
  async function remove(p: Provider) {
    if (!confirm(`Delete “${p.name}”? Movies it already added stay.`)) return;
    try {
      await del(`/importlist/${p.id}`);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function syncNow() {
    setSyncing(true);
    try {
      await post("/command", { name: "ImportListSync", wait: true });
      notify("Lists synced.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setSyncing(false);
    }
  }

  return (
    <div className="stack">
      <section className="card stack">
        <h2 style={{ margin: 0 }}>Import lists</h2>
        <p className="muted small">Lists add movies to the library automatically. They come from plugins and are read every six hours.</p>
        <LoadState loading={list.loading || plugins.loading} error={list.error ?? plugins.error} hasData={!!list.data && !!plugins.data} />
        {!draft &&
          list.data?.map((p) => (
            <div key={p.id} className="spread">
              <span>
                <strong>{p.name}</strong> <span className="badge">{p.plugin_id}</span> {!p.enabled && <span className="badge">off</span>}
              </span>
              <span className="row">
                <button onClick={() => edit(p)} disabled={!ready}>
                  Edit
                </button>
                <button className="danger" onClick={() => remove(p)}>
                  Delete
                </button>
              </span>
            </div>
          ))}
        {draft ? (
          <Editor
            draft={draft}
            onChange={setDraft}
            plugins={sources.map((p) => ({ id: p.id, label: p.name }))}
            profiles={profiles.data ?? []}
            roots={roots.data ?? []}
            onClose={(saved) => {
              setDraft(null);
              if (saved) list.reload();
            }}
          />
        ) : (
          <div className="row end">
            {sources.length === 0 && plugins.data && <span className="muted small">No list plugins are installed.</span>}
            {sources.length > 0 && !ready && <span className="muted small">Add a root folder first.</span>}
            <button onClick={syncNow} disabled={syncing || !list.data?.length}>
              {syncing ? "Syncing…" : "Sync now"}
            </button>
            <button
              disabled={sources.length === 0 || !ready}
              onClick={() =>
                setDraft({
                  id: 0,
                  name: "",
                  plugin_id: sources[0].id,
                  enabled: true,
                  quality_profile_id: profiles.data![0].id,
                  root_folder_path: roots.data![0].path,
                  monitored: true,
                  search_on_add: false,
                  minimum_availability: "announced",
                  series_quality_profile_id: null,
                  series_root_folder_path: "",
                  series_monitor: "all",
                  settings: {},
                })
              }
            >
              Add list
            </button>
          </div>
        )}
      </section>
      <Exclusions />
    </div>
  );
}

function Editor({
  draft,
  onChange,
  plugins,
  profiles,
  roots,
  onClose,
}: {
  draft: Draft;
  onChange: (d: Draft) => void;
  plugins: { id: string; label: string }[];
  profiles: QualityProfile[];
  roots: RootFolder[];
  onClose: (saved: boolean) => void;
}) {
  const notify = useToast();
  const [schema, setSchema] = useState<Schema | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; error?: string } | null>(null);
  const set = (patch: Partial<Draft>) => onChange({ ...draft, ...patch });

  useEffect(() => {
    let cancelled = false;
    setSchema(null);
    get<Schema>(`/plugin/${draft.plugin_id}/schema`)
      .then((s) => {
        if (cancelled) return;
        setSchema(s);
        if (!draft.id && Object.keys(draft.settings).length === 0) onChange({ ...draft, settings: schemaDefaults(s) });
      })
      .catch((e) => !cancelled && notify(errorMessage(e), "bad"));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft.plugin_id]);

  const body = () => ({
    name: draft.name,
    implementation: draft.plugin_id,
    plugin_id: draft.plugin_id,
    enabled: draft.enabled,
    settings: {
      quality_profile_id: draft.quality_profile_id,
      root_folder_path: draft.root_folder_path,
      monitored: draft.monitored,
      search_on_add: draft.search_on_add,
      minimum_availability: draft.minimum_availability,
      ...(draft.series_quality_profile_id != null && draft.series_root_folder_path
        ? { series_quality_profile_id: draft.series_quality_profile_id, series_root_folder_path: draft.series_root_folder_path, series_monitor: draft.series_monitor }
        : {}),
      settings: draft.settings,
    },
  });

  async function test() {
    setBusy(true);
    try {
      setResult(await post("/importlist/test", body()));
    } catch (e) {
      setResult({ ok: false, error: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    try {
      if (draft.id) await put(`/importlist/${draft.id}`, body());
      else await post("/importlist", body());
      onClose(true);
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack">
      <div className="form-grid">
        <label className="field">
          <span>Name</span>
          <input type="text" value={draft.name} onChange={(e) => set({ name: e.target.value })} autoFocus />
        </label>
        <label className="field">
          <span>Plugin</span>
          <select value={draft.plugin_id} disabled={!!draft.id} onChange={(e) => set({ plugin_id: e.target.value, settings: {} })}>
            {plugins.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {schema && <SchemaForm schema={schema} value={draft.settings} onChange={(settings) => set({ settings })} />}
      <h3>New movies</h3>
      <div className="form-grid">
        <label className="field">
          <span>Quality profile</span>
          <select value={draft.quality_profile_id} onChange={(e) => set({ quality_profile_id: Number(e.target.value) })}>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Root folder</span>
          <select value={draft.root_folder_path} onChange={(e) => set({ root_folder_path: e.target.value })}>
            {roots.map((r) => (
              <option key={r.id} value={r.path}>
                {r.path}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Minimum availability</span>
          <select value={draft.minimum_availability} onChange={(e) => set({ minimum_availability: e.target.value })}>
            <option value="announced">Announced</option>
            <option value="inCinemas">In cinemas</option>
            <option value="released">Released</option>
          </select>
        </label>
      </div>
      <h3>New series</h3>
      <p className="muted small">Only needed when the list names series. Leave the profile empty to skip them.</p>
      <div className="form-grid">
        <label className="field">
          <span>Quality profile</span>
          <select value={draft.series_quality_profile_id ?? ""} onChange={(e) => set({ series_quality_profile_id: e.target.value ? Number(e.target.value) : null })}>
            <option value="">Skip series</option>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Root folder</span>
          <select value={draft.series_root_folder_path} onChange={(e) => set({ series_root_folder_path: e.target.value })}>
            <option value="">Choose…</option>
            {roots.map((r) => (
              <option key={r.id} value={r.path}>
                {r.path}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Monitor</span>
          <select value={draft.series_monitor} onChange={(e) => set({ series_monitor: e.target.value })}>
            <option value="all">All episodes</option>
            <option value="future">Future episodes</option>
            <option value="missing">Missing episodes</option>
            <option value="first_season">First season</option>
            <option value="pilot">Pilot only</option>
            <option value="none">None</option>
          </select>
        </label>
      </div>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={draft.monitored} onChange={(e) => set({ monitored: e.target.checked })} /> Monitor them
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.search_on_add} onChange={(e) => set({ search_on_add: e.target.checked })} /> Search right away
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.enabled} onChange={(e) => set({ enabled: e.target.checked })} /> Enabled
        </label>
      </div>
      {result && (result.ok ? <p className="good">The list answered.</p> : <p className="error">{result.error ?? "Failed."}</p>)}
      <div className="row end">
        <button onClick={() => onClose(false)} disabled={busy}>
          Cancel
        </button>
        <button onClick={test} disabled={busy || !schema}>
          Test
        </button>
        <button className="primary" onClick={save} disabled={busy || !draft.name.trim() || !schema}>
          Save
        </button>
      </div>
    </div>
  );
}

function Exclusions() {
  const notify = useToast();
  const list = useLoad(() => get<Exclusion[]>("/exclusion"));
  if (list.data?.length === 0) return null;
  async function remove(e: Exclusion) {
    try {
      await del(`/exclusion/${e.tmdb_id}`);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Exclusions</h2>
      <p className="muted small">Lists never add these movies. Remove one to let lists add it again.</p>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data?.map((e) => (
        <div key={e.tmdb_id} className="spread">
          <span>
            {e.title} {e.year > 0 && <span className="muted">({e.year})</span>}
          </span>
          <button onClick={() => remove(e)}>Remove</button>
        </div>
      ))}
    </section>
  );
}
