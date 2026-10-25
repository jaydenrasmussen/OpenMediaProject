"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { LoadState } from "../components/Page";
import { Backdrop } from "../components/Hero";
import { Poster } from "../components/Poster";
import { Releases } from "../components/Releases";
import { TagPicker } from "../components/TagPicker";
import { TitleExtras } from "../components/TitleExtras";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { formatDate, formatRuntime, formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import { useLive } from "../lib/live";
import type { HistoryItem, Movie, QualityProfile, Tag } from "../lib/types";

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
  useLive(() => {
    movie.reload();
    history.reload();
  }, ["history"]);
  const [showSearch, setShowSearch] = useState(false);
  const [editing, setEditing] = useState(false);

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
          <Backdrop images={m.images} />
          <div className="detail over">
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
                  {m.media_servers.length > 0 && <span className="badge good">On {m.media_servers.join(", ")}</span>}
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
                <button onClick={() => setEditing((v) => !v)}>{editing ? "Close settings" : "Settings"}</button>
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
                <span className="row" style={{ marginLeft: "auto" }}>
                  {m.has_file && (
                    <button className="danger" onClick={deleteFile}>
                      Delete file
                    </button>
                  )}
                  <button className="danger" onClick={() => setRemoving((r) => !r)}>
                    Remove
                  </button>
                </span>
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

              {editing && <MovieSettings movie={m} onSaved={() => movie.reload()} />}

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

          {showSearch && (
            <Releases
              searchPath={`/release?movie_id=${m.id}`}
              grabPath="/release"
              canGrab={(r) => r.matched_movie_id != null}
              onGrabbed={() => history.reload()}
            />
          )}

          <TitleExtras kind="movie" tmdbId={m.tmdb_id} />

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

function MovieSettings({ movie, onSaved }: { movie: Movie; onSaved: () => void }) {
  const notify = useToast();
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const allTags = useLoad(() => get<Tag[]>("/tag"));
  const [profile, setProfile] = useState(movie.quality_profile_id);
  const [availability, setAvailability] = useState(movie.minimum_availability);
  const [tags, setTags] = useState(movie.tags);
  const [busy, setBusy] = useState(false);

  async function save() {
    setBusy(true);
    try {
      await put(`/movie/${movie.id}`, { quality_profile_id: profile, minimum_availability: availability, tags });
      notify("Saved.", "good");
      onSaved();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card stack">
      <div className="form-grid">
        <label className="field">
          <span>Quality profile</span>
          <select value={profile} onChange={(e) => setProfile(Number(e.target.value))}>
            {profiles.data?.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Minimum availability</span>
          <select value={availability} onChange={(e) => setAvailability(e.target.value)}>
            <option value="announced">Announced</option>
            <option value="inCinemas">In cinemas</option>
            <option value="released">Released</option>
          </select>
        </label>
      </div>
      <div>
        <span className="small muted">Tags</span>
        <TagPicker all={allTags.data ?? []} value={tags} onChange={setTags} empty="No tags yet. Add some in Settings, then pick them here." />
      </div>
      <div className="row end">
        <button className="primary" onClick={save} disabled={busy}>
          Save
        </button>
      </div>
    </section>
  );
}
