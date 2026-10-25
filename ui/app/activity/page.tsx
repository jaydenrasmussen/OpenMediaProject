"use client";

import Link from "next/link";
import { useState } from "react";
import { ManualImport } from "../components/ManualImport";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { formatDate, formatDuration, formatSize, percent } from "../lib/format";
import { errorMessage, useLoad, usePoll } from "../lib/hooks";
import { useLive } from "../lib/live";
import type { BlocklistItem, EpisodeHistoryItem, HistoryItem, QueueItem, TvBlocklistItem } from "../lib/types";

const STATE_BADGE: Record<string, string> = {
  Downloading: "accent",
  ImportPending: "warn",
  ImportBlocked: "bad",
  Importing: "accent",
  Imported: "good",
  Failed: "bad",
  FailedPending: "bad",
};

const EVENT_LABEL: Record<string, string> = {
  grabbed: "Grabbed",
  downloadFolderImported: "Imported",
  downloadFailed: "Download failed",
  movieFileDeleted: "File deleted",
  movieFolderImported: "Folder imported",
  movieFileRenamed: "Renamed",
  downloadIgnored: "Ignored",
  episodeFileDeleted: "File deleted",
  seriesFolderImported: "Folder imported",
  episodeFileRenamed: "Renamed",
};

export default function Activity() {
  const [tab, setTab] = useState<"queue" | "history" | "blocklist">("queue");
  return (
    <>
      <h1>Activity</h1>
      <div className="tabs" role="tablist">
        <button role="tab" className={tab === "queue" ? "active" : ""} onClick={() => setTab("queue")}>
          Queue
        </button>
        <button role="tab" className={tab === "history" ? "active" : ""} onClick={() => setTab("history")}>
          History
        </button>
        <button role="tab" className={tab === "blocklist" ? "active" : ""} onClick={() => setTab("blocklist")}>
          Blocklist
        </button>
      </div>
      {tab === "queue" ? <Queue /> : tab === "history" ? <History /> : <Blocklist />}
    </>
  );
}

function Queue() {
  const notify = useToast();
  const q = useLoad(() => get<QueueItem[]>("/queue"));
  useLive(q.reload, ["queue"]);
  usePoll(q.reload, 30000); // a safety net if the socket is down

  const [removing, setRemoving] = useState<QueueItem | null>(null);
  const [importing, setImporting] = useState<string | null>(null);
  const [blocklist, setBlocklist] = useState(true);
  const [deleteData, setDeleteData] = useState(false);

  async function control(i: QueueItem, what: "pause" | "resume") {
    try {
      await post(`/queue/${encodeURIComponent(i.download_id)}/${what}`);
      q.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  async function prioritise(i: QueueItem, priority: string) {
    try {
      await put(`/queue/${encodeURIComponent(i.download_id)}/priority`, { priority });
      q.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  async function confirmRemove() {
    if (!removing) return;
    try {
      await del(`/queue/${encodeURIComponent(removing.download_id)}?blocklist=${blocklist}&search=${blocklist}&delete_data=${deleteData}`);
      setRemoving(null);
      q.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <>
      <LoadState loading={q.loading} error={q.error} hasData={!!q.data} />
      {q.data && q.data.length === 0 && <p className="muted">Nothing is downloading.</p>}
      {q.data && q.data.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Title</th>
                <th>Client</th>
                <th>State</th>
                <th>Progress</th>
                <th className="num">Size</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {q.data.map((i) => (
                <tr key={i.download_id}>
                  <td style={{ wordBreak: "break-word" }}>
                    {i.movie_id ? (
                      <Link href={`/movie/?id=${i.movie_id}`}>{i.title}</Link>
                    ) : i.series_id ? (
                      <Link href={`/series/show/?id=${i.series_id}`}>{i.title}</Link>
                    ) : (
                      i.title
                    )}
                    {i.message && <div className="small error">{i.message}</div>}
                    {i.warnings.map((w) => (
                      <div key={w} className="small warn">
                        {w}
                      </div>
                    ))}
                  </td>
                  <td className="small">
                    {i.client}
                    <div className="muted">{i.protocol}</div>
                  </td>
                  <td>
                    <span className={`badge ${STATE_BADGE[i.state] ?? ""}`}>{i.state.replace(/([a-z])([A-Z])/g, "$1 $2")}</span>
                  </td>
                  <td style={{ minWidth: 110 }}>
                    <div className="progress">
                      <div style={{ width: `${percent(i.size, i.size_left)}%` }} />
                    </div>
                    <span className="small muted">
                      {percent(i.size, i.size_left)}%
                      {i.download_speed > 0 && <> · {formatSize(i.download_speed)}/s</>}
                      {i.eta_seconds != null && <> · {formatDuration(i.eta_seconds)}</>}
                    </span>
                  </td>
                  <td className="num">{formatSize(i.size)}</td>
                  <td className="row">
                    {i.status === "Paused" ? (
                      <button onClick={() => control(i, "resume")}>Resume</button>
                    ) : i.status === "Downloading" || i.status === "Queued" ? (
                      <button onClick={() => control(i, "pause")}>Pause</button>
                    ) : null}
                    {(i.status === "Downloading" || i.status === "Queued" || i.status === "Paused") && (
                      <select value={i.priority} onChange={(e) => prioritise(i, e.target.value)} aria-label={`Priority of ${i.title}`}>
                        <option value="low">Low</option>
                        <option value="normal">Normal</option>
                        <option value="high">High</option>
                        <option value="force">Force</option>
                      </select>
                    )}
                    {(i.state === "ImportBlocked" || i.state === "ImportPending") && (
                      <button onClick={() => setImporting(i.download_id)} aria-label={`Import ${i.title} by hand`}>
                        Manual import
                      </button>
                    )}
                    <button className="danger" onClick={() => setRemoving(i)} aria-label={`Remove ${i.title}`}>
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {importing && (
        <ManualImport
          downloadId={importing}
          onClose={(done) => {
            setImporting(null);
            if (done) q.reload();
          }}
        />
      )}
      {removing && (
        <section className="card stack" style={{ marginTop: "1rem" }}>
          <strong>Remove “{removing.title}” from the queue?</strong>
          <label className="check">
            <input type="checkbox" checked={blocklist} onChange={(e) => setBlocklist(e.target.checked)} /> Blocklist this release and search for another
          </label>
          <label className="check">
            <input type="checkbox" checked={deleteData} onChange={(e) => setDeleteData(e.target.checked)} /> Delete the downloaded files
          </label>
          <div className="row end">
            <button onClick={() => setRemoving(null)}>Cancel</button>
            <button className="danger" onClick={confirmRemove}>
              Remove
            </button>
          </div>
        </section>
      )}
    </>
  );
}

type BlockRow = { key: string; tv: boolean; id: number; link: string; source_title: string; message: string | null; quality: string; indexer: string | null; date: string };

function Blocklist() {
  const notify = useToast();
  const b = useLoad(async () => {
    const [movies, series] = await Promise.all([get<BlocklistItem[]>("/blocklist?limit=200"), get<TvBlocklistItem[]>("/blocklist/series?limit=200")]);
    const rows: BlockRow[] = [
      ...movies.map((m) => ({ key: `m${m.id}`, tv: false, id: m.id, link: `/movie/?id=${m.movie_id}`, source_title: m.source_title, message: m.message, quality: m.quality, indexer: m.indexer, date: m.date })),
      ...series.map((t) => ({ key: `s${t.id}`, tv: true, id: t.id, link: `/series/show/?id=${t.series_id}`, source_title: t.source_title, message: t.message, quality: t.quality, indexer: t.indexer, date: t.date })),
    ];
    return rows.sort((a, c) => c.date.localeCompare(a.date));
  });
  useLive(b.reload, []);
  async function remove(i: BlockRow) {
    try {
      await del(i.tv ? `/blocklist/series/${i.id}` : `/blocklist/${i.id}`);
      b.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function clear() {
    if (!confirm("Clear the whole blocklist? Those releases can be grabbed again.")) return;
    try {
      await del("/blocklist");
      await del("/blocklist/series");
      b.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  return (
    <>
      <LoadState loading={b.loading} error={b.error} hasData={!!b.data} />
      {b.data && b.data.length === 0 && <p className="muted">Nothing is blocklisted. Failed downloads end up here so they are not grabbed again.</p>}
      {b.data && b.data.length > 0 && (
        <>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Release</th>
                  <th>Quality</th>
                  <th>Indexer</th>
                  <th>When</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {b.data.map((i) => (
                  <tr key={i.key}>
                    <td style={{ wordBreak: "break-word" }}>
                      <Link href={i.link}>{i.source_title}</Link>
                      {i.message && <div className="small error">{i.message}</div>}
                    </td>
                    <td>{i.quality}</td>
                    <td className="small">{i.indexer ?? "—"}</td>
                    <td className="small muted">{formatDate(i.date)}</td>
                    <td>
                      <button onClick={() => remove(i)}>Unblock</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="row end" style={{ marginTop: "1rem" }}>
            <button className="danger" onClick={clear}>
              Clear blocklist
            </button>
          </div>
        </>
      )}
    </>
  );
}

const PAGE = 50;

type HistoryRow = { key: string; link: string; label?: string; event: string; source_title: string; message?: string; quality: string; date: string };

function History() {
  const [page, setPage] = useState(0);
  const h = useLoad(async () => {
    const q = `limit=${PAGE}&offset=${page * PAGE}`;
    const [movies, episodes] = await Promise.all([get<HistoryItem[]>(`/history?${q}`), get<EpisodeHistoryItem[]>(`/history/episode?${q}`)]);
    const rows: HistoryRow[] = [
      ...movies.map((m) => ({ key: `m${m.id}`, link: `/movie/?id=${m.movie_id}`, event: m.event, source_title: m.source_title, message: m.data.message, quality: m.quality, date: m.date })),
      ...episodes.map((e) => ({
        key: `e${e.id}`,
        link: `/series/show/?id=${e.series_id}`,
        label: `${e.series_title ?? ""} ${e.season_number}x${String(e.episode_number ?? 0).padStart(2, "0")}`,
        event: e.event,
        source_title: e.source_title,
        message: e.data.message,
        quality: e.quality,
        date: e.date,
      })),
    ];
    return rows.sort((a, b) => b.date.localeCompare(a.date)).slice(0, PAGE);
  }, [page]);
  useLive(h.reload, ["history"]);
  return (
    <>
      <LoadState loading={h.loading} error={h.error} hasData={!!h.data} />
      {h.data && h.data.length === 0 && <p className="muted">{page === 0 ? "No history yet." : "No more history."}</p>}
      {h.data && h.data.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Event</th>
                <th>Release</th>
                <th>Quality</th>
                <th>When</th>
              </tr>
            </thead>
            <tbody>
              {h.data.map((i) => (
                <tr key={i.key}>
                  <td>
                    <span className={`badge ${i.event === "downloadFailed" ? "bad" : i.event === "grabbed" ? "accent" : "good"}`}>
                      {EVENT_LABEL[i.event] ?? i.event}
                    </span>
                  </td>
                  <td style={{ wordBreak: "break-word" }}>
                    <Link href={i.link}>{i.source_title}</Link>
                    {i.label && <div className="small muted">{i.label}</div>}
                    {i.message && <div className="small error">{i.message}</div>}
                  </td>
                  <td>{i.quality}</td>
                  <td className="small muted">{formatDate(i.date)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="row" style={{ marginTop: "1rem" }}>
        <button disabled={page === 0} onClick={() => setPage((p) => p - 1)}>
          Newer
        </button>
        <button disabled={!h.data || h.data.length < PAGE} onClick={() => setPage((p) => p + 1)}>
          Older
        </button>
        <span className="muted small">Page {page + 1}</span>
      </div>
    </>
  );
}
