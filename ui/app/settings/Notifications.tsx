"use client";

import { useEffect, useState } from "react";
import { LoadState } from "../components/Page";
import { SchemaForm, schemaDefaults } from "../components/SchemaForm";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { PluginList, Provider, Schema } from "../lib/types";

const TRIGGERS = [
  { id: "grab", label: "A release is grabbed" },
  { id: "import", label: "A movie is imported" },
  { id: "download-failed", label: "A download fails" },
];

interface Draft {
  id: number;
  name: string;
  plugin_id: string;
  enabled: boolean;
  triggers: string[];
  settings: Record<string, unknown>;
}

type Stored = { triggers?: string[]; settings?: Record<string, unknown> };

export function Notifications() {
  const notify = useToast();
  const list = useLoad(() => get<Provider[]>("/notification"));
  const plugins = useLoad(() => get<PluginList>("/plugin"));
  const [draft, setDraft] = useState<Draft | null>(null);
  const notifiers = plugins.data?.plugins.filter((p) => p.kind === "notification") ?? [];

  function edit(p: Provider) {
    const stored = p.settings as Stored;
    setDraft({ id: p.id, name: p.name, plugin_id: p.plugin_id ?? "", enabled: p.enabled, triggers: stored.triggers ?? [], settings: stored.settings ?? {} });
  }
  async function remove(p: Provider) {
    if (!confirm(`Delete “${p.name}”?`)) return;
    try {
      await del(`/notification/${p.id}`);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Notifications</h2>
      <p className="muted small">Notifications come from plugins. Install a plugin by putting its folder in the plugins folder, then restart.</p>
      <LoadState loading={list.loading || plugins.loading} error={list.error ?? plugins.error} hasData={!!list.data && !!plugins.data} />
      {plugins.data?.problems.map((p) => (
        <p key={p} className="error small">
          {p}
        </p>
      ))}
      {!draft &&
        list.data?.map((p) => (
          <div key={p.id} className="spread">
            <span>
              <strong>{p.name}</strong> <span className="badge">{p.plugin_id}</span> {!p.enabled && <span className="badge">off</span>}
            </span>
            <span className="row">
              <button onClick={() => edit(p)}>Edit</button>
              <button className="danger" onClick={() => remove(p)}>
                Delete
              </button>
            </span>
          </div>
        ))}
      {draft ? (
        <Editor
          draft={draft}
          plugins={notifiers.map((p) => ({ id: p.id, label: p.name }))}
          onChange={setDraft}
          onClose={(saved) => {
            setDraft(null);
            if (saved) list.reload();
          }}
        />
      ) : (
        <div className="row end">
          {notifiers.length === 0 && plugins.data && <span className="muted small">No notification plugins are installed.</span>}
          <button
            disabled={notifiers.length === 0}
            onClick={() => setDraft({ id: 0, name: "", plugin_id: notifiers[0].id, enabled: true, triggers: ["grab", "import", "download-failed"], settings: {} })}
          >
            Add notification
          </button>
        </div>
      )}
    </section>
  );
}

function Editor({ draft, plugins, onChange, onClose }: { draft: Draft; plugins: { id: string; label: string }[]; onChange: (d: Draft) => void; onClose: (saved: boolean) => void }) {
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
        // A new notification starts from the plugin's defaults.
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
    settings: { triggers: draft.triggers, settings: draft.settings },
  });

  async function test() {
    setBusy(true);
    try {
      setResult(await post("/notification/test", body()));
    } catch (e) {
      setResult({ ok: false, error: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    try {
      if (draft.id) await put(`/notification/${draft.id}`, body());
      else await post("/notification", body());
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
      <fieldset style={{ border: 0, padding: 0 }}>
        <legend className="muted small">Notify me when</legend>
        <div className="row">
          {TRIGGERS.map((t) => (
            <label key={t.id} className="check">
              <input
                type="checkbox"
                checked={draft.triggers.includes(t.id)}
                onChange={(e) => set({ triggers: e.target.checked ? [...draft.triggers, t.id] : draft.triggers.filter((x) => x !== t.id) })}
              />{" "}
              {t.label}
            </label>
          ))}
        </div>
      </fieldset>
      <label className="check">
        <input type="checkbox" checked={draft.enabled} onChange={(e) => set({ enabled: e.target.checked })} /> Enabled
      </label>
      {result && (result.ok ? <p className="good">Sent.</p> : <p className="error">{result.error ?? "Failed."}</p>)}
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
