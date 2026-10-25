"use client";

import { useState } from "react";
import { useToast } from "../components/toast";
import { del, get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { PluginInfo, PluginList } from "../lib/types";

// What each kind of plugin is for and which settings tab uses it, in the order
// people reach for them. Within a group the popular plugins come first.
const GROUPS: { kind: PluginInfo["kind"]; title: string; where: string; popular: string[] }[] = [
  { kind: "download_client", title: "Other download clients (optional)", where: "Settings → Download clients", popular: ["qbittorrent", "sabnzbd", "transmission", "nzbget"] },
  { kind: "media_server", title: "Media servers: see what you already have", where: "Settings → Media servers", popular: ["plex-library", "jellyfin-library"] },
  { kind: "notification", title: "Notifications", where: "Settings → Notifications", popular: ["discord", "telegram", "slack", "pushover", "ntfy", "gotify", "plex", "jellyfin-emby", "webhook"] },
  { kind: "list", title: "Import lists", where: "Settings → Import lists", popular: ["custom-list"] },
  { kind: "indexer", title: "Indexers", where: "Settings → Indexers", popular: [] },
];

function ordered(plugins: PluginInfo[], popular: string[]) {
  const rank = (id: string) => (popular.includes(id) ? popular.indexOf(id) : popular.length);
  return [...plugins].sort((a, b) => rank(a.id) - rank(b.id) || a.name.localeCompare(b.name));
}

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

interface Source {
  id: string;
  name: string;
  version: string;
  url: string;
  installed: boolean;
}

export function Plugins() {
  const notify = useToast();
  const list = useLoad(() => get<PluginList>("/plugin"));
  const sources = useLoad(() => get<Source[]>("/plugin/sources"));
  // Plugins a restored backup remembers but this install does not have yet.
  const missing = sources.data?.filter((s) => !s.installed) ?? [];
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
      sources.reload();
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
      sources.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  const p = preview?.plugin;
  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Plugins</h2>
      <p className="muted small">
        Plugins add download clients, media servers, notifications, import lists and indexers. They run in a sandbox and can only reach the web hosts they list. Install one from the address of its zip file.
      </p>
      {GROUPS.map((g) => {
        const here = ordered(list.data?.plugins.filter((pl) => pl.kind === g.kind) ?? [], g.popular);
        if (!here.length) return null;
        return (
          <div key={g.kind} className="stack">
            <div className="spread">
              <h3 style={{ margin: 0 }}>{g.title}</h3>
              <span className="muted small">Set up in {g.where}</span>
            </div>
            {g.kind === "download_client" && (
              <p className="muted small" style={{ margin: 0 }}>
                You do not need these. Usenet and BitTorrent downloading are built in: add “Built-in usenet” or “Built-in torrent” under Settings → Download clients. Use a plugin below only to hand downloads to a client you already run.
              </p>
            )}
            <div className="plugin-grid">
              {here.map((pl) => (
                <div key={pl.id} className="plugin-card">
                  <div className="spread">
                    <strong>{pl.name}</strong>
                    {pl.removable ? (
                      <button className="danger quiet" onClick={() => remove(pl.id, pl.name)}>
                        Remove
                      </button>
                    ) : (
                      <span className="badge good">Built in</span>
                    )}
                  </div>
                  <div className="small">{pl.description}</div>
                  <div className="small muted">
                    {pl.version} · May reach: {pl.allowed_hosts.length ? pl.allowed_hosts.join(", ") : "no hosts"}
                  </div>
                </div>
              ))}
            </div>
          </div>
        );
      })}
      {missing.length > 0 && (
        <div className="banner warn stack">
          <strong>Plugins this setup used but are not installed</strong>
          {missing.map((s) => (
            <div key={s.id} className="spread">
              <span>
                {s.name} <span className="muted small">{s.version}</span>
              </span>
              <button
                onClick={() => {
                  setUrl(s.url);
                  setPreview(null);
                }}
              >
                Use its address
              </button>
            </div>
          ))}
        </div>
      )}
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
