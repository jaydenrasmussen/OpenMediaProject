"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post } from "../lib/api";
import { formatDate, formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { BackupInfo, HealthIssue, LogLine, SystemStatus, TaskStatus } from "../lib/types";

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
                <th>Metadata (TMDB)</th>
                <td>{status.data.tmdb_configured ? "Configured" : "Not configured"}</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}
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
        A copy of the database is made every week and the last seven are kept. To restore one, stop the app, put the file in the data folder as <code>omp.db</code> (delete <code>omp.db-wal</code> and <code>omp.db-shm</code> next to it), and start it again.
      </p>
      <LoadState loading={backups.loading} error={backups.error} hasData={!!backups.data} />
      {backups.data?.length === 0 && <p className="muted">No backups yet.</p>}
      {backups.data?.map((b) => (
        <div key={b.name} className="card spread" style={{ marginBottom: ".5rem" }}>
          <div>
            <a href={`/api/v1/system/backup/${encodeURIComponent(b.name)}`} download>
              {b.name}
            </a>
            <div className="muted small">
              {formatDate(b.created)} · {formatSize(b.size)}
            </div>
          </div>
          <button className="danger" onClick={() => remove(b)}>
            Delete
          </button>
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
