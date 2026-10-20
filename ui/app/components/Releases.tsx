"use client";

import { useState } from "react";
import { LoadState } from "./Page";
import { useToast } from "./toast";
import { get, post } from "../lib/api";
import { formatAge, formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Release, ReleaseSearch } from "../lib/types";

/**
 * The results of an interactive search with a Grab button per release.
 * `searchPath` returns a ReleaseSearch; `grabPath` takes `{ id }`.
 */
export function Releases({
  searchPath,
  grabPath,
  title = "Interactive search",
  canGrab,
  onGrabbed,
}: {
  searchPath: string;
  grabPath: string;
  title?: string;
  canGrab: (r: Release) => boolean;
  onGrabbed: () => void;
}) {
  const notify = useToast();
  const search = useLoad(() => get<ReleaseSearch>(searchPath), [searchPath]);
  const [busy, setBusy] = useState<string | null>(null);
  const [showRejected, setShowRejected] = useState(true);

  async function grab(r: Release) {
    if (!r.approved && !confirm(`The engine rejected this release:\n\n${r.rejections.join("\n")}\n\nGrab it anyway?`)) return;
    setBusy(r.id);
    try {
      const res = await post<{ client: string }>(grabPath, { id: r.id });
      notify(`Sent to ${res.client}.`, "good");
      onGrabbed();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(null);
    }
  }

  const releases = (search.data?.releases ?? []).filter((r) => showRejected || r.approved);
  const tv = releases.some((r) => r.episodes !== undefined);
  return (
    <>
      <div className="spread">
        <h2>{title}</h2>
        <div className="row">
          <label className="check small">
            <input type="checkbox" checked={showRejected} onChange={(e) => setShowRejected(e.target.checked)} /> Show rejected
          </label>
          <button onClick={search.reload} disabled={search.loading}>
            {search.loading ? "Searching…" : "Search again"}
          </button>
        </div>
      </div>
      <LoadState loading={search.loading} error={search.error} hasData={!!search.data} />
      {search.data?.indexer_failures.map((f) => (
        <p key={f.indexer} className="warn small">
          {f.indexer}: {f.error}
        </p>
      ))}
      {search.data && releases.length === 0 && !search.loading && (
        <p className="muted">No releases found. Check that an indexer is configured with interactive search enabled.</p>
      )}
      {releases.length > 0 && (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Release</th>
                {tv && <th>Episodes</th>}
                <th>Indexer</th>
                <th>Quality</th>
                <th className="num">Size</th>
                <th className="num">Age</th>
                <th className="num">Peers</th>
                <th className="num">Score</th>
                <th>Status</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {releases.map((r) => (
                <tr key={r.id}>
                  <td style={{ wordBreak: "break-word" }}>{r.title}</td>
                  {tv && <td className="small">{r.episodes?.join(", ")}</td>}
                  <td className="small">{r.indexer}</td>
                  <td>{r.quality ?? "?"}</td>
                  <td className="num">{formatSize(r.size)}</td>
                  <td className="num">{formatAge(r.age_hours)}</td>
                  <td className="num">{r.seeders != null ? `${r.seeders}/${r.peers ?? 0}` : "–"}</td>
                  <td className="num">{r.custom_format_score}</td>
                  <td>
                    {r.approved ? (
                      <span className="badge good">{r.rank === 1 ? "Best" : "OK"}</span>
                    ) : (
                      <span className={`badge ${r.temporarily_rejected ? "warn" : "bad"}`} title={r.rejections.join("\n")}>
                        {r.rejections[0] ?? "Rejected"}
                        {r.rejections.length > 1 ? ` +${r.rejections.length - 1}` : ""}
                      </span>
                    )}
                  </td>
                  <td>
                    <button className={r.approved ? "primary" : ""} onClick={() => grab(r)} disabled={busy !== null || !canGrab(r)}>
                      {busy === r.id ? "…" : r.approved ? "Grab" : "Grab anyway"}
                    </button>
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
