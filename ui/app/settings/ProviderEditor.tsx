"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Provider } from "../lib/types";

export interface Implementation {
  id: string;
  label: string;
  defaults: Record<string, unknown>;
  Fields: React.ComponentType<{
    settings: Record<string, unknown>;
    onChange: (s: Record<string, unknown>) => void;
  }>;
  /** Short description for the list, from the settings. */
  describe: (s: Record<string, unknown>) => string;
  /** Set when a plugin provides this implementation. */
  pluginId?: string;
}

interface Props {
  path: string; // "indexer" | "downloadclient"
  noun: string; // "indexer"
  implementations: Implementation[];
  /** Show the RSS / search switches (indexers only). */
  searchSwitches?: boolean;
  help?: React.ReactNode;
}

type Draft = Omit<Provider, "id"> & { id?: number };

export function ProviderManager({ path, noun, implementations, searchSwitches, help }: Props) {
  const notify = useToast();
  const list = useLoad(() => get<Provider[]>(`/${path}`));
  const [draft, setDraft] = useState<Draft | null>(null);

  function startNew() {
    const impl = implementations[0];
    setDraft({
      name: "",
      implementation: impl.id,
      plugin_id: impl.pluginId ?? null,
      settings: structuredClone(impl.defaults),
      tags: [],
      enabled: true,
      enable_rss: true,
      enable_automatic_search: true,
      enable_interactive_search: true,
      priority: 25,
    });
  }

  async function remove(p: Provider) {
    if (!confirm(`Delete ${noun} “${p.name}”?`)) return;
    try {
      await del(`/${path}/${p.id}`);
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  async function toggle(p: Provider) {
    try {
      await put(`/${path}/${p.id}`, { ...p, enabled: !p.enabled });
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  return (
    <div className="stack">
      {help && <p className="muted small">{help}</p>}
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data?.length === 0 && !draft && <p className="muted">No {noun}s yet.</p>}
      {list.data?.map((p) => {
        const impl = implementations.find((i) => i.id === p.implementation);
        return (
          <div key={p.id} className="card spread">
            <div>
              <strong>{p.name}</strong> <span className="badge">{impl?.label ?? p.implementation}</span>{" "}
              {!p.enabled && <span className="badge warn">Disabled</span>}
              <div className="muted small">{impl?.describe(p.settings) ?? ""}</div>
            </div>
            <div className="row">
              <button onClick={() => toggle(p)}>{p.enabled ? "Disable" : "Enable"}</button>
              <button onClick={() => setDraft({ ...p, settings: structuredClone(p.settings) })}>Edit</button>
              <button className="danger" onClick={() => remove(p)}>
                Delete
              </button>
            </div>
          </div>
        );
      })}
      {draft ? (
        <Editor
          key={draft.id ?? "new"}
          path={path}
          noun={noun}
          draft={draft}
          setDraft={setDraft}
          implementations={implementations}
          searchSwitches={searchSwitches}
          onClose={() => setDraft(null)}
          onSaved={() => {
            setDraft(null);
            list.reload();
          }}
        />
      ) : (
        <div>
          <button className="primary" onClick={startNew}>
            Add {noun}
          </button>
        </div>
      )}
    </div>
  );
}

function Editor({
  path,
  noun,
  draft,
  setDraft,
  implementations,
  searchSwitches,
  onClose,
  onSaved,
}: {
  path: string;
  noun: string;
  draft: Draft;
  setDraft: (d: Draft) => void;
  implementations: Implementation[];
  searchSwitches?: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const notify = useToast();
  const [busy, setBusy] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; error?: string } | null>(null);
  const impl = implementations.find((i) => i.id === draft.implementation) ?? implementations[0];
  const set = (patch: Partial<Draft>) => {
    setTestResult(null);
    setDraft({ ...draft, ...patch });
  };

  async function test() {
    setBusy(true);
    setTestResult(null);
    try {
      setTestResult(await post<{ ok: boolean; error?: string }>(`/${path}/test`, draft));
    } catch (e) {
      setTestResult({ ok: false, error: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    setBusy(true);
    try {
      if (draft.id) await put(`/${path}/${draft.id}`, draft);
      else await post(`/${path}`, draft);
      notify("Saved.", "good");
      onSaved();
    } catch (e) {
      notify(errorMessage(e), "bad");
      setBusy(false);
    }
  }

  return (
    <section className="card stack">
      <h3 style={{ margin: 0 }}>{draft.id ? `Edit ${noun}` : `Add ${noun}`}</h3>
      <div className="form-grid">
        <label className="field">
          <span>Name</span>
          <input type="text" value={draft.name} onChange={(e) => set({ name: e.target.value })} autoFocus />
        </label>
        <label className="field">
          <span>Type</span>
          <select
            value={draft.implementation}
            disabled={!!draft.id}
            onChange={(e) => {
              const next = implementations.find((i) => i.id === e.target.value)!;
              set({ implementation: next.id, plugin_id: next.pluginId ?? null, settings: structuredClone(next.defaults) });
            }}
          >
            {implementations.map((i) => (
              <option key={i.id} value={i.id}>
                {i.label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Priority (1 = first)</span>
          <input type="number" min={1} max={50} value={draft.priority} onChange={(e) => set({ priority: Number(e.target.value) })} />
        </label>
      </div>

      <impl.Fields settings={draft.settings} onChange={(settings) => set({ settings })} />

      <div className="row">
        <label className="check">
          <input type="checkbox" checked={draft.enabled} onChange={(e) => set({ enabled: e.target.checked })} /> Enabled
        </label>
        {searchSwitches && (
          <>
            <label className="check">
              <input type="checkbox" checked={draft.enable_rss} onChange={(e) => set({ enable_rss: e.target.checked })} /> RSS sync
            </label>
            <label className="check">
              <input type="checkbox" checked={draft.enable_automatic_search} onChange={(e) => set({ enable_automatic_search: e.target.checked })} /> Automatic search
            </label>
            <label className="check">
              <input type="checkbox" checked={draft.enable_interactive_search} onChange={(e) => set({ enable_interactive_search: e.target.checked })} /> Interactive search
            </label>
          </>
        )}
      </div>

      {testResult && (testResult.ok ? <p className="good">Works.</p> : <p className="error">{testResult.error ?? "Failed."}</p>)}
      <div className="row end">
        <button onClick={onClose} disabled={busy}>
          Cancel
        </button>
        <button onClick={test} disabled={busy}>
          Test
        </button>
        <button className="primary" onClick={save} disabled={busy || !draft.name.trim()}>
          Save
        </button>
      </div>
    </section>
  );
}
