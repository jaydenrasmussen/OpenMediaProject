"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post } from "../lib/api";
import { formatDate, formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { BackupInfo, HealthIssue, SystemStatus } from "../lib/types";

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

      <Backups />

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
