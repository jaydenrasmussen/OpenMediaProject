"use client";

import { useState } from "react";
import { useToast } from "../components/toast";
import { del, get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { PluginList } from "../lib/types";

interface Preview {
  installed: boolean;
  plugin: {
    id: string;
    name: string;
    version: string;
    kind: string;
    description: string;
    allowed_hosts: string[];
    sha256: string;
    installed_version: string | null;
  };
}

export function Plugins() {
  const notify = useToast();
  const list = useLoad(() => get<PluginList>("/plugin"));
  const [url, setUrl] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);

  async function look() {
    setBusy(true);
    setPreview(null);
    try {
      setPreview(await post<Preview>("/plugin/install", { url: url.trim() }));
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  async function install() {
    if (!preview) return;
    setBusy(true);
    try {
      await post("/plugin/install", { url: url.trim(), sha256: preview.plugin.sha256 });
      notify(`Installed ${preview.plugin.name}.`, "good");
      setPreview(null);
      setUrl("");
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string, name: string) {
    if (!confirm(`Remove “${name}”? Indexers, lists and notifications that use it stop working until it is installed again.`)) return;
    try {
      await del(`/plugin/${id}`);
      list.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  const p = preview?.plugin;
  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Plugins</h2>
      <p className="muted small">
        Plugins add indexers, notifications and import lists. They run in a sandbox and can only reach the web hosts they list. Install one from the address of its zip file.
      </p>
      {list.data?.plugins.map((pl) => (
        <div key={pl.id} className="spread">
          <span>
            <strong>{pl.name}</strong> <span className="muted small">{pl.version} · {pl.kind}</span>
            <div className="small muted">{pl.description}</div>
            <div className="small muted">May reach: {pl.allowed_hosts.length ? pl.allowed_hosts.join(", ") : "no hosts"}</div>
          </span>
          {pl.removable ? (
            <button className="danger" onClick={() => remove(pl.id, pl.name)}>
              Remove
            </button>
          ) : (
            <span className="badge">Bundled</span>
          )}
        </div>
      ))}
      {list.data?.problems.map((m) => (
        <p key={m} className="warn small">
          {m}
        </p>
      ))}
      <label className="field">
        <span>Install from a URL</span>
        <input type="text" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com/my-plugin.zip" />
      </label>
      <div className="row end">
        <button onClick={look} disabled={busy || !url.trim()}>
          {busy && !preview ? "Looking…" : "Look at it"}
        </button>
      </div>
      {p && (
        <div className="card stack">
          <strong>
            {p.name} {p.version} <span className="muted small">({p.kind})</span>
          </strong>
          <div className="small">{p.description}</div>
          <div className="small">
            <strong>It will be allowed to reach:</strong> {p.allowed_hosts.length ? p.allowed_hosts.join(", ") : "no hosts"}
            {p.allowed_hosts.includes("*") && <span className="warn"> (any host)</span>}
          </div>
          {p.installed_version && <div className="small warn">Version {p.installed_version} is installed and will be replaced.</div>}
          <div className="small muted">
            Package fingerprint: <code>{p.sha256.slice(0, 16)}…</code>
          </div>
          <div className="row end">
            <button onClick={() => setPreview(null)}>Cancel</button>
            <button className="primary" onClick={install} disabled={busy}>
              {p.installed_version ? "Replace it" : "Install"}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
