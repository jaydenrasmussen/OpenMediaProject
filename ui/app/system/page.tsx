"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { apiUrl, del, get, post } from "../lib/api";
import { formatDate, formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { BackupInfo, HealthIssue, LogLine, RootFolder, SystemStatus, TaskStatus, VpnStatus } from "../lib/types";

export default function System() {
  const health = useLoad(() => get<HealthIssue[]>("/system/health"));
  const status = useLoad(() => get<SystemStatus>("/system/status"));
  return (
    <>
      <div className="spread">
        <h1>System</h1>
        <button onClick={health.reload} disabled={health.loading}>
          Check again
        </button>
      </div>

      <h2>Health</h2>
      <LoadState loading={health.loading} error={health.error} hasData={!!health.data} />
      {health.data && health.data.length === 0 && <p className="good">Everything looks fine.</p>}
      <div className="stack">
        {health.data?.map((i, n) => (
          <div key={n} className="card">
            <span className={`badge ${i.level === "error" ? "bad" : "warn"}`}>{i.level}</span>{" "}
            <span className="muted small">{i.source}</span>
            <div>{i.message}</div>
          </div>
        ))}
      </div>

      <Vpn />

      <DiskSpace status={status.data ?? null} />

      <Tasks />

      <Backups />

      <Logs />

      <h2>About</h2>
      <LoadState loading={status.loading} error={status.error} hasData={!!status.data} />
      {status.data && (
        <div className="card">
          <table>
            <tbody>
              <tr>
                <th>Version</th>
                <td>{status.data.version}</td>
              </tr>
              <tr>
                <th>Started</th>
                <td>{formatDate(status.data.started)}</td>
              </tr>
              <tr>
                <th>Data folder</th>
                <td>
                  <code>{status.data.data_dir}</code>
                </td>
              </tr>
              <tr>
                <th>Metadata</th>
                <td>{status.data.tmdb_token_set ? "Radarr’s metadata server, with TMDB as the fallback" : "Radarr’s metadata server (add a TMDB token for series and a fallback)"}</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

/** Free space where the library, the downloads and the app's own data live. */
function DiskSpace({ status }: { status: SystemStatus | null }) {
  const roots = useLoad(() => get<RootFolder[]>("/rootfolder"));
  if (!status) return null;
  const rows: { label: string; path: string; free: number | null }[] = [
    ...(roots.data ?? []).map((r) => ({ label: "Library", path: r.path, free: r.free_space })),
    { label: "Downloads", path: status.downloads_dir, free: status.downloads_dir_free },
    { label: "App data", path: status.data_dir, free: status.data_dir_free },
  ];
  return (
    <>
      <h2>Disk space</h2>
      <div className="card">
        <table>
          <tbody>
            {rows.map((r) => (
              <tr key={`${r.label}${r.path}`}>
                <th>{r.label}</th>
                <td>
                  <code>{r.path}</code>
                </td>
                <td>{r.free == null ? "unknown" : `${formatSize(r.free)} free`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

function Backups() {
  const notify = useToast();
  const backups = useLoad(() => get<BackupInfo[]>("/system/backup"));
  const [busy, setBusy] = useState(false);

  async function make() {
    setBusy(true);
    try {
      await post("/system/backup");
      backups.reload();
      notify("Backup created.", "good");
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }
  const [staged, setStaged] = useState<string | null>(null);
  async function restore(b: BackupInfo) {
    if (!confirm(`Restore ${b.name}?\n\nThe library, settings and history go back to how they were then. Your current database is kept as omp.db.before-restore. The change happens when the app restarts.`)) return;
    try {
      await post(`/system/backup/${encodeURIComponent(b.name)}/restore`);
      setStaged(b.name);
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  async function restart() {
    try {
      await post("/system/restart");
      notify("Restarting. Reload the page in a few seconds.", "good");
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  async function remove(b: BackupInfo) {
    if (!confirm(`Delete ${b.name}?`)) return;
    try {
      await del(`/system/backup/${encodeURIComponent(b.name)}`);
      backups.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  return (
    <>
      <div className="spread">
        <h2>Backups</h2>
        <button onClick={make} disabled={busy}>
          {busy ? "Working…" : "Back up now"}
        </button>
      </div>
      <p className="muted small">
        A copy of the database is made every week and the last seven are kept. Restoring one takes effect when the app restarts.
      </p>
      {staged && (
        <div className="card spread" style={{ marginBottom: ".5rem" }}>
          <span>
            <strong>{staged}</strong> will be restored when the app restarts.
          </span>
          <button className="primary" onClick={restart}>
            Restart now
          </button>
        </div>
      )}
      <LoadState loading={backups.loading} error={backups.error} hasData={!!backups.data} />
      {backups.data?.length === 0 && <p className="muted">No backups yet.</p>}
      {backups.data?.map((b) => (
        <div key={b.name} className="card spread" style={{ marginBottom: ".5rem" }}>
          <div>
            <a href={apiUrl(`/system/backup/${encodeURIComponent(b.name)}`)} download>
              {b.name}
            </a>
            <div className="muted small">
              {formatDate(b.created)} · {formatSize(b.size)}
            </div>
          </div>
          <div className="row">
            <button onClick={() => restore(b)}>Restore</button>
            <button className="danger" onClick={() => remove(b)}>
              Delete
            </button>
          </div>
        </div>
      ))}
    </>
  );
}

function Tasks() {
  const notify = useToast();
  const tasks = useLoad(() => get<TaskStatus[]>("/system/task"));
  const RUN_NOW: Record<string, string> = {
    "rss-sync": "RssSync",
    "process-downloads": "ProcessDownloads",
    "refresh-movies": "RefreshMovies",
    "import-lists": "ImportListSync",
    "media-servers": "MediaServerSync",
  };
  async function run(name: string) {
    try {
      await post("/command", { name: RUN_NOW[name], wait: true });
      notify("Done.", "good");
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      tasks.reload();
    }
  }
  const every = (secs: number) => (secs >= 3600 ? `${Math.round(secs / 3600)} h` : secs >= 60 ? `${Math.round(secs / 60)} min` : `${secs} s`);
  return (
    <>
      <h2>Tasks</h2>
      <LoadState loading={tasks.loading} error={tasks.error} hasData={!!tasks.data} />
      {tasks.data && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Task</th>
                <th>Every</th>
                <th>Last run</th>
                <th>Next run</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {tasks.data.map((t) => (
                <tr key={t.name}>
                  <td>
                    {t.label}
                    {t.last_error && <div className="small error">{t.last_error}</div>}
                  </td>
                  <td>{every(t.interval_secs)}</td>
                  <td className="small muted">{t.last_run ? formatDate(t.last_run) : "not yet"}</td>
                  <td className="small muted">{t.next_run ? formatDate(t.next_run) : "—"}</td>
                  <td>{RUN_NOW[t.name] && <button onClick={() => run(t.name)}>Run now</button>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

const LEVEL_BADGE: Record<string, string> = { error: "bad", warn: "warn", info: "", debug: "" };

function Logs() {
  const notify = useToast();
  const [level, setLevel] = useState("info");
  const [search, setSearch] = useState("");
  const logs = useLoad(() => get<LogLine[]>(`/log?level=${level}&limit=200&search=${encodeURIComponent(search)}`), [level, search]);

  async function clear() {
    if (!confirm("Delete the whole event log?")) return;
    try {
      await del("/log");
      logs.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  return (
    <>
      <div className="spread">
        <h2>Event log</h2>
        <div className="row">
          <input type="search" placeholder="Search" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search the log" />
          <select value={level} onChange={(e) => setLevel(e.target.value)} aria-label="Minimum level">
            <option value="error">Errors</option>
            <option value="warn">Warnings and errors</option>
            <option value="info">Everything</option>
          </select>
          <button onClick={logs.reload}>Refresh</button>
          <a className="button" href={apiUrl("/log/download")} download="omp.log">
            Download
          </a>
          <button className="danger" onClick={clear}>
            Clear
          </button>
        </div>
      </div>
      <LoadState loading={logs.loading} error={logs.error} hasData={!!logs.data} />
      {logs.data && logs.data.length === 0 && <p className="muted">Nothing logged.</p>}
      {logs.data && logs.data.length > 0 && (
        <div className="table-wrap">
          <table>
            <tbody>
              {logs.data.map((l, i) => (
                <tr key={i}>
                  <td className="small muted" style={{ whiteSpace: "nowrap" }}>
                    {formatDate(l.time)}
                  </td>
                  <td>
                    <span className={`badge ${LEVEL_BADGE[l.level] ?? ""}`}>{l.level}</span>
                  </td>
                  <td style={{ wordBreak: "break-word" }}>
                    {l.message}
                    <div className="small muted">{l.logger}</div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

/** Shown only when the container runs its own VPN (Gluetun). */
function Vpn() {
  const vpn = useLoad(() => get<VpnStatus>("/system/vpn"));
  if (!vpn.data?.enabled) return null;
  const v = vpn.data;
  const up = v.status === "running";
  const where = [v.city, v.region, v.country].filter(Boolean).join(", ");
  return (
    <>
      <h2>VPN</h2>
      <div className="card stack">
        <div className="spread">
          <span>
            <span className={`badge ${up ? "good" : "bad"}`}>{up ? "Connected" : (v.status ?? "unknown")}</span>{" "}
            {v.provider && <span className="muted">{v.provider}</span>}
          </span>
          <button onClick={vpn.reload} disabled={vpn.loading}>
            Refresh
          </button>
        </div>
        {up && (
          <div className="muted">
            Traffic leaves from {v.public_ip ?? "an unknown address"}
            {where ? ` (${where})` : ""}.
          </div>
        )}
      </div>
    </>
  );
}
