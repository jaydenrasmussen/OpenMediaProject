"use client";

import Link from "next/link";
import { useState } from "react";
import { LoadState } from "../components/Page";
import { get } from "../lib/api";
import { formatDate, formatSize, percent } from "../lib/format";
import { useLoad, usePoll } from "../lib/hooks";
import type { HistoryItem, QueueItem } from "../lib/types";

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
  const [tab, setTab] = useState<"queue" | "history">("queue");
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
      </div>
      {tab === "queue" ? <Queue /> : <History />}
    </>
  );
}

function Queue() {
  const q = useLoad(() => get<QueueItem[]>("/queue"));
  usePoll(q.reload, 4000);
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
                </tr>
              ))}
            </tbody>
          </table>
        </div>
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
