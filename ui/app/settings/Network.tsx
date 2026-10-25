"use client";

import { useEffect, useState } from "react";
import { useToast } from "../components/toast";
import { get, post, put } from "../lib/api";
import { errorMessage } from "../lib/hooks";

interface Host {
  port: number | null;
  url_base: string;
  ssl: { enabled: boolean; cert_path: string; key_path: string };
  proxy: { enabled: boolean; scheme: string; host: string; port: number; username: string; has_password: boolean; bypass: string };
  log: { level: string; keep_days: number; max_rows: number };
  tmdb: { custom: boolean; from_environment: boolean };
  running: { port: number; url_base: string; ssl: boolean; port_from_environment: boolean; url_base_from_environment: boolean };
}

/** Port, URL base, HTTPS and an outbound proxy. These are read at start-up. */
export function Network() {
  const notify = useToast();
  const [host, setHost] = useState<Host | null>(null);
  const [password, setPassword] = useState("");
  const [tmdbToken, setTmdbToken] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    get<Host>("/config/host")
      .then(setHost)
      .catch((e) => notify(errorMessage(e), "bad"));
  }, [notify]);

  if (!host) return <p className="muted">Loading…</p>;

  const change = (patch: Partial<Host>) => setHost({ ...host, ...patch });
  const proxy = (patch: Partial<Host["proxy"]>) => change({ proxy: { ...host.proxy, ...patch } });
  const log = (patch: Partial<Host["log"]>) => change({ log: { ...host.log, ...patch } });
  const ssl = (patch: Partial<Host["ssl"]>) => change({ ssl: { ...host.ssl, ...patch } });

  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!host) return;
    setBusy(true);
    try {
      const saved = await put<Host>("/config/host", {
        port: host.port,
        url_base: host.url_base,
        ssl: host.ssl,
        log: host.log,
        tmdb_token: tmdbToken.trim() === "" ? undefined : tmdbToken.trim(),
        proxy: { ...host.proxy, password: password === "" ? undefined : password },
      });
      setHost(saved);
      setPassword("");
      setTmdbToken("");
      notify("Saved. Restart for this to take effect.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setBusy(false);
    }
  }

  async function useBuiltInToken() {
    if (!host) return;
    setBusy(true);
    try {
      setHost(await put<Host>("/config/host", { port: host.port, url_base: host.url_base, ssl: host.ssl, log: host.log, proxy: { ...host.proxy, password: undefined }, tmdb_token: "" }));
      setTmdbToken("");
      notify("Back to the built-in key. Restart for this to take effect.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setBusy(false);
    }
  }

  async function restart() {
    if (!confirm("Restart now? The page is unavailable for a few seconds.")) return;
    try {
      await post("/system/restart");
      notify("Restarting…");
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  const r = host.running;
  return (
    <form className="card stack" onSubmit={save}>
      <h2 style={{ margin: 0 }}>Network</h2>
      <p className="muted small">
        Changes here are read when the app starts. Running now: port {r.port}
        {r.url_base ? `, URL base ${r.url_base}` : ""}, {r.ssl ? "HTTPS" : "plain HTTP"}.
      </p>

      <div className="form-grid">
        <label className="field">
          <span>Port{r.port_from_environment ? " (set by OMP_ADDR, which wins)" : ""}</span>
          <input
            type="number"
            min={1}
            max={65535}
            placeholder="8080"
            value={host.port ?? ""}
            onChange={(e) => change({ port: e.target.value === "" ? null : Number(e.target.value) })}
          />
        </label>
        <label className="field">
          <span>URL base{r.url_base_from_environment ? " (set by OMP_URL_BASE, which wins)" : ""}</span>
          <input placeholder="/media" value={host.url_base} onChange={(e) => change({ url_base: e.target.value })} />
        </label>
      </div>
      <p className="muted small">The URL base is the folder a reverse proxy serves the app under, like /media.</p>

      <h3>HTTPS</h3>
      <label className="check">
        <input type="checkbox" checked={host.ssl.enabled} onChange={(e) => ssl({ enabled: e.target.checked })} /> Serve over HTTPS
      </label>
      {host.ssl.enabled && (
        <div className="form-grid">
          <label className="field">
            <span>Certificate file (PEM)</span>
            <input value={host.ssl.cert_path} onChange={(e) => ssl({ cert_path: e.target.value })} placeholder="/config/cert.pem" />
          </label>
          <label className="field">
            <span>Private key file (PEM)</span>
            <input value={host.ssl.key_path} onChange={(e) => ssl({ key_path: e.target.value })} placeholder="/config/key.pem" />
          </label>
        </div>
      )}

      <h3>Proxy</h3>
      <label className="check">
        <input type="checkbox" checked={host.proxy.enabled} onChange={(e) => proxy({ enabled: e.target.checked })} /> Reach the internet through a proxy
      </label>
      {host.proxy.enabled && (
        <>
          <div className="form-grid">
            <label className="field">
              <span>Type</span>
              <select value={host.proxy.scheme} onChange={(e) => proxy({ scheme: e.target.value })}>
                <option value="http">HTTP</option>
                <option value="https">HTTPS</option>
                <option value="socks5">SOCKS5</option>
              </select>
            </label>
            <label className="field">
              <span>Address</span>
              <input value={host.proxy.host} onChange={(e) => proxy({ host: e.target.value })} />
            </label>
            <label className="field">
              <span>Port</span>
              <input type="number" min={1} max={65535} value={host.proxy.port} onChange={(e) => proxy({ port: Number(e.target.value) })} />
            </label>
            <label className="field">
              <span>Username</span>
              <input value={host.proxy.username} onChange={(e) => proxy({ username: e.target.value })} autoComplete="off" />
            </label>
            <label className="field">
              <span>Password{host.proxy.has_password ? " (leave empty to keep the saved one)" : ""}</span>
              <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
            </label>
            <label className="field">
              <span>Skip the proxy for</span>
              <input value={host.proxy.bypass} onChange={(e) => proxy({ bypass: e.target.value })} />
            </label>
          </div>
          <p className="muted small">Covers indexers, metadata, artwork, plugins and downloads of .torrent and .nzb files. Torrent peers and usenet servers are reached directly.</p>
        </>
      )}

      <h3>Movie and series information (TMDB)</h3>
      <p className="muted small">
        Posters, banners, cast and release dates come from TMDB. A shared read-only key is built in, so this works without any set-up. If you would rather use your own, paste your “API Read Access Token” from
        themoviedb.org (it starts with eyJ).
      </p>
      {host.tmdb.from_environment ? (
        <p className="small">The TMDB_READ_TOKEN environment variable is set, so it is used and this field is ignored.</p>
      ) : (
        <div className="row" style={{ alignItems: "flex-end" }}>
          <label className="field grow">
            <span>{host.tmdb.custom ? "Using your own token. Paste a new one to replace it" : "Using the built-in key. Your own token"}</span>
            <input type="password" autoComplete="off" placeholder="eyJ…" value={tmdbToken} onChange={(e) => setTmdbToken(e.target.value)} />
          </label>
          {host.tmdb.custom && (
            <button type="button" onClick={useBuiltInToken} disabled={busy}>
              Use the built-in key
            </button>
          )}
        </div>
      )}

      <h3>Event log</h3>
      <div className="form-grid">
        <label className="field">
          <span>Detail</span>
          <select value={host.log.level} onChange={(e) => log({ level: e.target.value })}>
            <option value="info">Normal</option>
            <option value="debug">Debug</option>
            <option value="trace">Everything</option>
          </select>
        </label>
        <label className="field">
          <span>Keep for (days)</span>
          <input type="number" min={1} max={365} value={host.log.keep_days} onChange={(e) => log({ keep_days: Number(e.target.value) })} />
        </label>
        <label className="field">
          <span>Keep at most (entries)</span>
          <input type="number" min={1000} max={1000000} step={1000} value={host.log.max_rows} onChange={(e) => log({ max_rows: Number(e.target.value) })} />
        </label>
      </div>
      <p className="muted small">The log takes effect as soon as you save. Older entries are removed every hour.</p>

      <div className="row end">
        <button type="button" onClick={restart}>
          Restart
        </button>
        <button className="primary" disabled={busy}>
          Save
        </button>
      </div>
    </form>
  );
}
