"use client";

import Link from "next/link";
import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get } from "../lib/api";
import { formatDate, formatSize, percent } from "../lib/format";
import { errorMessage, useLoad, usePoll } from "../lib/hooks";
import type { BlocklistItem, HistoryItem, QueueItem } from "../lib/types";

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
  usePoll(q.reload, 4000);

  const [removing, setRemoving] = useState<QueueItem | null>(null);
  const [blocklist, setBlocklist] = useState(true);
  const [deleteData, setDeleteData] = useState(false);

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
                    {i.movie_id ? <Link href={`/movie/?id=${i.movie_id}`}>{i.title}</Link> : i.title}
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
                    <span className="small muted">{percent(i.size, i.size_left)}%</span>
                  </td>
                  <td className="num">{formatSize(i.size)}</td>
                  <td>
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

function Blocklist() {
  const notify = useToast();
  const b = useLoad(() => get<BlocklistItem[]>("/blocklist?limit=200"));
  async function remove(i: BlocklistItem) {
    try {
      await del(`/blocklist/${i.id}`);
      b.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function clear() {
    if (!confirm("Clear the whole blocklist? Those releases can be grabbed again.")) return;
    try {
      await del("/blocklist");
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
                  <tr key={i.id}>
                    <td style={{ wordBreak: "break-word" }}>
                      <Link href={`/movie/?id=${i.movie_id}`}>{i.source_title}</Link>
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

function History() {
  const [page, setPage] = useState(0);
  const h = useLoad(() => get<HistoryItem[]>(`/history?limit=${PAGE}&offset=${page * PAGE}`), [page]);
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
                <tr key={i.id}>
                  <td>
                    <span className={`badge ${i.event === "downloadFailed" ? "bad" : i.event === "grabbed" ? "accent" : "good"}`}>
                      {EVENT_LABEL[i.event] ?? i.event}
                    </span>
                  </td>
                  <td style={{ wordBreak: "break-word" }}>
                    <Link href={`/movie/?id=${i.movie_id}`}>{i.source_title}</Link>
                    {i.data.message && <div className="small error">{i.data.message}</div>}
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
