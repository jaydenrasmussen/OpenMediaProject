"use client";

import { ProviderManager, type Implementation } from "./ProviderEditor";

interface Server {
  host: string;
  port?: number | null;
  tls: boolean;
  username?: string | null;
  password?: string | null;
  connections: number;
  priority: number;
}

function TorrentFields({ settings, onChange }: { settings: Record<string, unknown>; onChange: (s: Record<string, unknown>) => void }) {
  const set = (k: string, v: unknown) => onChange({ ...settings, [k]: v });
  return (
    <div className="form-grid">
      <label className="field">
        <span>Download folder (empty for the default)</span>
        <input type="text" value={String(settings.download_dir ?? "")} onChange={(e) => set("download_dir", e.target.value || null)} />
      </label>
      <label className="field">
        <span>Seed until ratio (0 = do not seed)</span>
        <input type="number" min={0} step={0.1} value={Number(settings.seed_ratio_limit ?? 0)} onChange={(e) => set("seed_ratio_limit", Number(e.target.value))} />
      </label>
      <label className="check" style={{ alignSelf: "end" }}>
        <input type="checkbox" checked={settings.enable_dht !== false} onChange={(e) => set("enable_dht", e.target.checked)} /> Use DHT
      </label>
    </div>
  );
}

function UsenetFields({ settings, onChange }: { settings: Record<string, unknown>; onChange: (s: Record<string, unknown>) => void }) {
  const servers = (settings.servers as Server[] | undefined) ?? [];
  const update = (next: Server[]) => onChange({ ...settings, servers: next });
  const patch = (i: number, p: Partial<Server>) => update(servers.map((s, n) => (n === i ? { ...s, ...p } : s)));
  return (
    <div className="stack">
      {servers.map((s, i) => (
        <fieldset key={i} className="card" style={{ margin: 0 }}>
          <legend className="small muted">Server {i + 1}</legend>
          <div className="form-grid">
            <label className="field">
              <span>Host</span>
              <input type="text" placeholder="news.example.com" value={s.host} onChange={(e) => patch(i, { host: e.target.value.trim() })} />
            </label>
            <label className="field">
              <span>Port (empty = 563 / 119)</span>
              <input type="number" min={1} max={65535} value={s.port ?? ""} onChange={(e) => patch(i, { port: e.target.value ? Number(e.target.value) : null })} />
            </label>
            <label className="field">
              <span>Username</span>
              <input type="text" autoComplete="off" value={s.username ?? ""} onChange={(e) => patch(i, { username: e.target.value || null })} />
            </label>
            <label className="field">
              <span>Password</span>
              <input type="password" autoComplete="new-password" value={s.password ?? ""} onChange={(e) => patch(i, { password: e.target.value || null })} />
            </label>
            <label className="field">
              <span>Connections</span>
              <input type="number" min={1} max={100} value={s.connections} onChange={(e) => patch(i, { connections: Number(e.target.value) })} />
            </label>
            <label className="field">
              <span>Priority (0 first; higher are backups)</span>
              <input type="number" min={0} value={s.priority} onChange={(e) => patch(i, { priority: Number(e.target.value) })} />
            </label>
          </div>
          <div className="spread" style={{ marginTop: ".5rem" }}>
            <label className="check">
              <input type="checkbox" checked={s.tls} onChange={(e) => patch(i, { tls: e.target.checked })} /> Use TLS
            </label>
            <button className="danger" onClick={() => update(servers.filter((_, n) => n !== i))} disabled={servers.length === 1}>
              Remove server
            </button>
          </div>
        </fieldset>
      ))}
      <div>
        <button onClick={() => update([...servers, { host: "", port: null, tls: true, username: null, password: null, connections: 8, priority: servers.length ? 1 : 0 }])}>
          Add server
        </button>
      </div>
    </div>
  );
}

const IMPLEMENTATIONS: Implementation[] = [
  {
    id: "usenet",
    label: "Built-in usenet",
    defaults: { servers: [{ host: "", port: null, tls: true, username: null, password: null, connections: 8, priority: 0 }] },
    Fields: UsenetFields,
    describe: (s) => ((s.servers as Server[] | undefined) ?? []).map((x) => x.host).filter(Boolean).join(", "),
  },
  {
    id: "torrent",
    label: "Built-in torrent",
    defaults: { download_dir: null, seed_ratio_limit: 0, enable_dht: true },
    Fields: TorrentFields,
    describe: (s) => (s.download_dir ? String(s.download_dir) : "default folder"),
  },
];

export function DownloadClients() {
  return (
    <ProviderManager
      path="downloadclient"
      noun="download client"
      implementations={IMPLEMENTATIONS}
      help="Downloads are handled in this app: the usenet client talks to your news servers and repairs and unpacks releases; the torrent client downloads over BitTorrent. Use Test to check server logins and folders."
    />
  );
}
