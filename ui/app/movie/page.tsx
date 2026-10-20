"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { formatAge, formatDate, formatRuntime, formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { HistoryItem, Movie, Release, ReleaseSearch } from "../lib/types";

export default function MoviePage() {
  // useSearchParams needs a Suspense boundary in a static export.
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <MovieDetail />
    </Suspense>
  );
}

const EVENT_LABEL: Record<string, string> = {
  grabbed: "Grabbed",
  downloadFolderImported: "Imported",
  downloadFailed: "Download failed",
  movieFileDeleted: "File deleted",
  movieFolderImported: "Folder imported",
  movieFileRenamed: "Renamed",
  downloadIgnored: "Ignored",
};

function MovieDetail() {
  const id = Number(useSearchParams().get("id"));
  const router = useRouter();
  const notify = useToast();
  const movie = useLoad(() => get<Movie>(`/movie/${id}`), [id]);
  const history = useLoad(() => get<HistoryItem[]>(`/history?movie_id=${id}`), [id]);
  const [showSearch, setShowSearch] = useState(false);

  if (!id) return <p className="error">No movie selected.</p>;
  const m = movie.data;

  async function toggleMonitored() {
    if (!m) return;
    try {
      await put(`/movie/${m.id}`, { monitored: !m.monitored });
      movie.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  async function automaticSearch() {
    try {
      await post("/command", { name: "MoviesSearch", movie_id: id });
      notify("Search started. Check Activity for the result.", "good");
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  async function runCommand(name: string, done: string) {
    try {
      await post("/command", { name, movie_id: id, wait: true });
      notify(done, "good");
      movie.reload();
      history.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  const [removing, setRemoving] = useState(false);
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [exclude, setExclude] = useState(false);

  async function remove() {
    if (!m) return;
    try {
      await del(`/movie/${m.id}?add_exclusion=${exclude}&delete_files=${deleteFiles}`);
      router.push("/");
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  async function deleteFile() {
    if (!m || !confirm(`Delete the file for “${m.title}”? The movie stays in the library.`)) return;
    try {
      await del(`/movie/${m.id}/file`);
      notify("File deleted.", "good");
      movie.reload();
      history.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  return (
    <>
      <LoadState loading={movie.loading} error={movie.error} hasData={!!m} />
      {m && (
        <>
          <div className="detail">
            <Poster images={m.images} title={m.title} width={500} />
            <div className="stack">
              <div>
                <h1 style={{ marginBottom: ".25rem" }}>
                  {m.title} <span className="muted">({m.year || "–"})</span>
                </h1>
                <div className="row small muted">
                  {m.runtime ? <span>{formatRuntime(m.runtime)}</span> : null}
                  {m.certification ? <span className="badge">{m.certification}</span> : null}
                  <span className="badge">{m.status}</span>
                  {m.genres.slice(0, 4).map((g) => (
                    <span key={g} className="badge">
                      {g}
                    </span>
                  ))}
                </div>
              </div>
              {m.overview && <p>{m.overview}</p>}

              <div className="row">
                <button className="primary" onClick={automaticSearch} disabled={!m.monitored} title={m.monitored ? "" : "Monitor the movie first"}>
                  Search
                </button>
                <button onClick={() => setShowSearch((s) => !s)}>{showSearch ? "Hide interactive search" : "Interactive search"}</button>
                <button onClick={toggleMonitored}>{m.monitored ? "Unmonitor" : "Monitor"}</button>
                <button onClick={() => runCommand("RescanMovie", "Folder scanned.")} title="Look at the movie folder for a file">
                  Rescan
                </button>
                {m.has_file && (
                  <Link href={`/rename/?id=${m.id}`} className="button">
                    Rename
                  </Link>
                )}
                <button onClick={() => runCommand("RefreshMovie", "Details refreshed.")} title="Re-read dates and details from TMDB">
                  Refresh
                </button>
                {m.has_file && (
                  <button className="danger" onClick={deleteFile}>
                    Delete file
                  </button>
                )}
                <button className="danger" onClick={() => setRemoving((r) => !r)}>
                  Remove
                </button>
              </div>
              {removing && (
                <section className="card stack">
                  <strong>Remove “{m.title}” from the library?</strong>
                  <label className="check">
                    <input type="checkbox" checked={deleteFiles} onChange={(e) => setDeleteFiles(e.target.checked)} /> Also delete its folder from disk
                  </label>
                  <label className="check">
                    <input type="checkbox" checked={exclude} onChange={(e) => setExclude(e.target.checked)} /> Stop import lists adding it again
                  </label>
                  <div className="row end">
                    <button onClick={() => setRemoving(false)}>Cancel</button>
                    <button className="danger" onClick={remove}>
                      Remove
                    </button>
                  </div>
                </section>
              )}

              <div className="card">
                <h3 style={{ marginTop: 0 }}>File</h3>
                {m.file ? (
                  <div className="small">
                    <div>
                      <code>{m.file.relative_path}</code>
                    </div>
                    <div className="row muted" style={{ marginTop: ".4rem" }}>
                      <span className="badge good">{m.file.quality}</span>
                      <span>{formatSize(m.file.size)}</span>
                      {m.file.release_group && <span>{m.file.release_group}</span>}
                      <span>added {formatDate(m.file.date_added)}</span>
                    </div>
                  </div>
                ) : (
                  <p className="muted small">
                    No file yet. {m.monitored ? "It is being watched for." : "It is not monitored."}
                  </p>
                )}
                <p className="muted small" style={{ marginBottom: 0 }}>
                  Folder: <code>{m.path}</code>
                </p>
              </div>
            </div>
          </div>

          {showSearch && <InteractiveSearch movieId={m.id} onGrabbed={() => history.reload()} />}

          <h2>History</h2>
          <LoadState loading={history.loading} error={history.error} hasData={!!history.data} />
          {history.data && history.data.length === 0 && <p className="muted">Nothing has happened yet.</p>}
          {history.data && history.data.length > 0 && (
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
                  {history.data.map((h) => (
                    <tr key={h.id}>
                      <td>
                        <span className={`badge ${h.event === "downloadFailed" ? "bad" : h.event === "grabbed" ? "accent" : "good"}`}>
                          {EVENT_LABEL[h.event] ?? h.event}
                        </span>
                      </td>
                      <td>
                        {h.source_title}
                        {h.data.message && <div className="small error">{h.data.message}</div>}
                      </td>
                      <td>{h.quality}</td>
                      <td className="small muted">{formatDate(h.date)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </>
  );
}

function InteractiveSearch({ movieId, onGrabbed }: { movieId: number; onGrabbed: () => void }) {
  const notify = useToast();
  const search = useLoad(() => get<ReleaseSearch>(`/release?movie_id=${movieId}`), [movieId]);
  const [busy, setBusy] = useState<string | null>(null);
  const [showRejected, setShowRejected] = useState(true);

  async function grab(r: Release) {
    if (!r.approved && !confirm(`The engine rejected this release:\n\n${r.rejections.join("\n")}\n\nGrab it anyway?`)) return;
    setBusy(r.id);
    try {
      const res = await post<{ client: string }>("/release", { id: r.id });
      notify(`Sent to ${res.client}.`, "good");
      onGrabbed();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(null);
    }
  }

  const releases = (search.data?.releases ?? []).filter((r) => showRejected || r.approved);
  return (
    <>
      <div className="spread">
        <h2>Interactive search</h2>
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
                    <button className={r.approved ? "primary" : ""} onClick={() => grab(r)} disabled={busy !== null || r.matched_movie_id == null}>
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
