"use client";

import { useEffect, useState } from "react";
import { LoadState } from "../components/Page";
import { SchemaForm, schemaDefaults } from "../components/SchemaForm";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { formatDate } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { MediaServerStatus, PluginList, Provider, Schema, Untracked } from "../lib/types";

interface Draft {
  id: number;
  name: string;
  plugin_id: string;
  enabled: boolean;
  settings: Record<string, unknown>;
}

export function MediaServers() {
  const notify = useToast();
  const list = useLoad(() => get<Provider[]>("/mediaserver"));
  const status = useLoad(() => get<MediaServerStatus[]>("/mediaserver/status"));
  const plugins = useLoad(() => get<PluginList>("/plugin"));
  const [draft, setDraft] = useState<Draft | null>(null);
  const [syncing, setSyncing] = useState(false);
  const servers = plugins.data?.plugins.filter((p) => p.kind === "media_server") ?? [];

  function edit(p: Provider) {
    setDraft({ id: p.id, name: p.name, plugin_id: p.plugin_id ?? "", enabled: p.enabled, settings: p.settings as Record<string, unknown> });
  }
  async function remove(p: Provider) {
    if (!confirm(`Remove “${p.name}”? Nothing is changed on the server itself.`)) return;
    try {
      await del(`/mediaserver/${p.id}`);
      list.reload();
      status.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function syncNow() {
    setSyncing(true);
    try {
      await post("/command", { name: "MediaServerSync", wait: true });
      notify("Libraries read.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setSyncing(false);
      status.reload();
    }
  }
  const statusOf = (id: number) => status.data?.find((s) => s.provider_id === id);

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Media servers</h2>
      <p className="muted small">
        Connect Plex, Jellyfin or Emby and this app reads what each library holds, so movies, series and episodes show what is already there. It only reads; nothing is changed on the server. Libraries are read every hour.
      </p>
      <LoadState loading={list.loading || plugins.loading} error={list.error ?? plugins.error} hasData={!!list.data && !!plugins.data} />
      {!draft &&
        list.data?.map((p) => {
          const s = statusOf(p.id);
          return (
            <div key={p.id} className="spread">
              <span>
                <strong>{p.name}</strong> <span className="badge">{p.plugin_id}</span> {!p.enabled && <span className="badge">off</span>}
                <div className="small muted">
                  {s?.error ? (
                    <span className="error">{s.error}</span>
                  ) : s?.synced_at ? (
                    `${s.movies} movies, ${s.series} series, ${s.episodes} episodes · read ${formatDate(s.synced_at)}`
                  ) : (
                    "Not read yet."
                  )}
                  {s?.error && s.synced_at ? ` Last good read ${formatDate(s.synced_at)}: ${s.movies} movies, ${s.series} series.` : ""}
                </div>
              </span>
              <span className="row">
                <button onClick={() => edit(p)}>Edit</button>
                <button className="danger" onClick={() => remove(p)}>
                  Remove
                </button>
              </span>
            </div>
          );
        })}
      {list.data?.length === 0 && !draft && <p className="muted">No media server is connected.</p>}
      {plugins.data && servers.length === 0 && !draft && <p className="muted small">No media server plugin is installed. Add one on the Plugins tab.</p>}
      {draft ? (
        <Editor
          draft={draft}
          plugins={servers.map((p) => ({ id: p.id, label: p.name }))}
          onChange={setDraft}
          onClose={(saved) => {
            setDraft(null);
            if (saved) {
              list.reload();
              // The new server is read in the background.
              setTimeout(status.reload, 1500);
            }
          }}
        />
      ) : (
        <div className="row end">
          {(list.data?.length ?? 0) > 0 && (
            <button onClick={syncNow} disabled={syncing}>
              {syncing ? "Reading…" : "Read libraries now"}
            </button>
          )}
          <button
            className="primary"
            disabled={servers.length === 0}
            onClick={() => setDraft({ id: 0, name: "", plugin_id: servers[0]?.id ?? "", enabled: true, settings: {} })}
          >
            Add media server
          </button>
        </div>
      )}
      {!draft && (list.data?.length ?? 0) > 0 && <Untracked />}
    </section>
  );
}

/** What the servers hold that this library does not. */
function Untracked() {
  const found = useLoad(() => get<Untracked>("/mediaserver/untracked"));
  const [open, setOpen] = useState(false);
  const movies = found.data?.movies ?? [];
  const series = found.data?.series ?? [];
  if (!found.data || movies.length + series.length === 0) return null;
  const line = (m: { tmdb_id: number; title: string; year: number | null; servers: string[] }) => (
    <li key={m.tmdb_id}>
      {m.title} {m.year ? <span className="muted">({m.year})</span> : null} <span className="muted small">on {m.servers.join(", ")}</span>
    </li>
  );
  return (
    <div className="stack">
      <button className="link" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        On your servers but not in this library: {movies.length} movies, {series.length} series
      </button>
      {open && (
        <>
          {movies.length > 0 && (
            <>
              <h3>Movies</h3>
              <ul>{movies.slice(0, 200).map(line)}</ul>
            </>
          )}
          {series.length > 0 && (
            <>
              <h3>Series</h3>
              <ul>{series.slice(0, 200).map(line)}</ul>
            </>
          )}
          {movies.length > 200 || series.length > 200 ? <p className="muted small">The first 200 of each are shown.</p> : null}
        </>
      )}
    </div>
  );
}

function Editor({ draft, plugins, onChange, onClose }: { draft: Draft; plugins: { id: string; label: string }[]; onChange: (d: Draft) => void; onClose: (saved: boolean) => void }) {
  const notify = useToast();
  const [schema, setSchema] = useState<Schema | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; error?: string; server?: string } | null>(null);
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

  const body = () => ({ name: draft.name, implementation: draft.plugin_id, plugin_id: draft.plugin_id, enabled: draft.enabled, settings: draft.settings });

  async function test() {
    setBusy(true);
    try {
      setResult(await post("/mediaserver/test", body()));
    } catch (e) {
      setResult({ ok: false, error: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    try {
      if (draft.id) await put(`/mediaserver/${draft.id}`, body());
      else await post("/mediaserver", body());
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
          <span>Server type</span>
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
      <label className="check">
        <input type="checkbox" checked={draft.enabled} onChange={(e) => set({ enabled: e.target.checked })} /> Enabled
      </label>
      {result && (result.ok ? <p className="good">Connected to {result.server || "the server"}.</p> : <p className="error">{result.error ?? "Failed."}</p>)}
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
