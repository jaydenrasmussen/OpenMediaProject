"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { LoadState } from "../../components/Page";
import { Poster } from "../../components/Poster";
import { Releases } from "../../components/Releases";
import { TagPicker } from "../../components/TagPicker";
import { useToast } from "../../components/toast";
import { del, get, post, put } from "../../lib/api";
import { formatDate, formatSize } from "../../lib/format";
import { errorMessage, useLoad } from "../../lib/hooks";
import { useLive } from "../../lib/live";
import type { Episode, EpisodeHistoryItem, QualityProfile, Series, Tag } from "../../lib/types";

export default function SeriesPage() {
  // useSearchParams needs a Suspense boundary in a static export.
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <SeriesDetail />
    </Suspense>
  );
}

const EVENT_LABEL: Record<string, string> = {
  grabbed: "Grabbed",
  downloadFolderImported: "Imported",
  downloadFailed: "Download failed",
  episodeFileDeleted: "File deleted",
  seriesFolderImported: "Folder imported",
  episodeFileRenamed: "Renamed",
  downloadIgnored: "Ignored",
};

function SeriesDetail() {
  const id = Number(useSearchParams().get("id"));
  const router = useRouter();
  const notify = useToast();
  const series = useLoad(() => get<Series>(`/series/${id}`), [id]);
  const episodes = useLoad(() => get<Episode[]>(`/episode?series_id=${id}`), [id]);
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const history = useLoad(() => get<EpisodeHistoryItem[]>(`/history/episode?series_id=${id}`), [id]);
  const [interactive, setInteractive] = useState<{ episode?: number; season?: number } | null>(null);
  const [removing, setRemoving] = useState(false);
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [editing, setEditing] = useState(false);
  const [open, setOpen] = useState<Set<number>>(new Set());

  if (!id) return <p className="error">No series selected.</p>;
  const s = series.data;
  const reload = () => {
    series.reload();
    episodes.reload();
    history.reload();
  };
  useLive(reload, ["history"]);

  async function run<T>(work: () => Promise<T>, done?: string) {
    try {
      await work();
      if (done) notify(done, "good");
      reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  const search = (command: Record<string, unknown>, done: string) =>
    run(() => post("/command", { ...command, series_id: id }), done);
  const searchMissing = () => search({ name: "SeriesSearch" }, "Search started. Check Activity for the result.");
  const searchSeason = (n: number) => search({ name: "SeasonSearch", season_number: n }, `Searching season ${n}.`);
  const searchEpisode = (e: Episode) => search({ name: "EpisodeSearch", episode_id: e.id }, `Searching ${e.title}.`);
  const rescan = () => run(() => post("/command", { name: "RescanSeries", series_id: id, wait: true }), "Folder scanned.");

  const toggleSeries = () => run(() => put(`/series/${id}`, { monitored: !s!.monitored }));
  const refresh = () => run(() => post("/command", { name: "RefreshSeries", series_id: id, wait: true }), "Details refreshed.");
  const toggleSeason = (n: number, monitored: boolean) => run(() => put(`/series/${id}/season/${n}`, { monitored }));
  const toggleEpisode = (e: Episode) => run(() => put("/episode/monitor", { episode_ids: [e.id], monitored: !e.monitored }));
  const deleteFile = (e: Episode) => {
    if (!e.file || !confirm(`Delete the file for ${e.title}?`)) return Promise.resolve();
    return run(() => del(`/episodefile/${e.file!.id}`), "File deleted.");
  };
  async function remove() {
    try {
      await del(`/series/${id}?delete_files=${deleteFiles}`);
      router.push("/series/");
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  const bySeason = new Map<number, Episode[]>();
  for (const e of episodes.data ?? []) bySeason.set(e.season_number, [...(bySeason.get(e.season_number) ?? []), e]);
  const seasonNumbers = [...bySeason.keys()].sort((a, b) => b - a);
  const latest = seasonNumbers.find((n) => n > 0);
  const isOpen = (n: number) => open.has(n) || (open.size === 0 && n === latest);
  const toggleOpen = (n: number) => {
    const next = new Set(open.size === 0 && latest !== undefined ? [latest] : open);
    if (next.has(n)) next.delete(n);
    else next.add(n);
    setOpen(next);
  };
  const profile = profiles.data?.find((p) => p.id === s?.quality_profile_id);

  return (
    <>
      <LoadState loading={series.loading} error={series.error} hasData={!!s} />
      {s && (
        <>
          <div className="detail">
            <Poster images={s.images} title={s.title} width={500} />
            <div className="stack">
              <div>
                <h1 style={{ marginBottom: ".2rem" }}>
                  {s.title} <span className="muted">({s.year || "?"})</span>
                </h1>
                <div className="row muted small">
                  <span className={`badge ${s.status === "continuing" ? "accent" : ""}`}>{s.status}</span>
                  {s.network && <span>{s.network}</span>}
                  {s.runtime > 0 && <span>{s.runtime} min</span>}
                  {s.certification && <span>{s.certification}</span>}
                  <span className="badge">{s.series_type}</span>
                  {s.genres.slice(0, 3).map((g) => (
                    <span key={g}>{g}</span>
                  ))}
                </div>
              </div>
              {s.overview && <p>{s.overview}</p>}
              <div className="row">
                <span className="badge good">
                  {s.episode_file_count}/{s.episode_count} episodes
                </span>
                {s.missing_episode_count > 0 && <span className="badge warn">{s.missing_episode_count} missing</span>}
                <span className="muted small">{formatSize(s.size_on_disk)}</span>
                {s.next_airing && <span className="muted small">Next: {formatDate(s.next_airing)}</span>}
              </div>
              <div className="row">
                <button onClick={toggleSeries}>{s.monitored ? "Unmonitor" : "Monitor"}</button>
                <button className="primary" onClick={searchMissing} disabled={!s.monitored} title={s.monitored ? "Look for every missing monitored episode" : "Monitor the series first"}>
                  Search missing
                </button>
                <button onClick={refresh}>Refresh</button>
                <button onClick={rescan} title="Look at the series folder for episode files">
                  Rescan
                </button>
                <Link href={`/rename/?series_id=${s.id}`} className="button">
                  Rename
                </Link>
                <button onClick={() => setEditing((v) => !v)}>{editing ? "Close settings" : "Settings"}</button>
                <button className="danger" onClick={() => setRemoving((v) => !v)}>
                  Remove
                </button>
              </div>
              {removing && (
                <section className="card stack">
                  <strong>Remove “{s.title}” from the library?</strong>
                  <label className="check">
                    <input type="checkbox" checked={deleteFiles} onChange={(e) => setDeleteFiles(e.target.checked)} /> Also delete its folder from disk
                  </label>
                  <div className="row end">
                    <button onClick={() => setRemoving(false)}>Cancel</button>
                    <button className="danger" onClick={remove}>
                      Remove
                    </button>
                  </div>
                </section>
              )}
              {editing && <SeriesSettings series={s} profiles={profiles.data ?? []} onSaved={reload} />}
              <div className="muted small">
                <code>{s.path}</code>
                {profile && <> · {profile.name}</>}
              </div>
            </div>
          </div>

          <LoadState loading={episodes.loading} error={episodes.error} hasData={!!episodes.data} />
          {seasonNumbers.map((n) => {
            const list = bySeason.get(n)!;
            const info = s.seasons.find((x) => x.season_number === n);
            return (
              <section key={n} className="card" style={{ marginTop: "1rem" }}>
                <div className="spread">
                  <button className="link" onClick={() => toggleOpen(n)} aria-expanded={isOpen(n)} style={{ background: "none", border: 0, padding: 0, textAlign: "left" }}>
                    <strong>{n === 0 ? "Specials" : `Season ${n}`}</strong>{" "}
                    <span className="muted small">
                      {info?.episode_file_count ?? 0}/{info?.episode_count ?? 0}
                    </span>
                  </button>
                  <div className="row">
                    <button onClick={() => searchSeason(n)} title="Search for a season pack or the missing episodes">
                      Search
                    </button>
                    <button onClick={() => setInteractive(interactive?.season === n ? null : { season: n })}>Interactive</button>
                    <label className="check">
                      <input type="checkbox" checked={info?.monitored ?? false} onChange={(e) => toggleSeason(n, e.target.checked)} /> Monitored
                    </label>
                  </div>
                </div>
                {interactive?.season === n && (
                  <Releases
                    searchPath={`/release/episode?series_id=${id}&season=${n}`}
                    grabPath="/release/episode"
                    title={`Interactive search: ${n === 0 ? "specials" : `season ${n}`}`}
                    canGrab={(r) => r.matched_series_id != null}
                    onGrabbed={reload}
                  />
                )}
                {interactive?.episode !== undefined && list.some((e) => e.id === interactive.episode) && (
                  <Releases
                    searchPath={`/release/episode?episode_id=${interactive.episode}`}
                    grabPath="/release/episode"
                    title="Interactive search"
                    canGrab={(r) => r.matched_series_id != null}
                    onGrabbed={reload}
                  />
                )}
                {isOpen(n) && (
                  <div className="table-wrap" style={{ marginTop: ".5rem" }}>
                    <table>
                      <thead>
                        <tr>
                          <th />
                          <th className="num">#</th>
                          <th>Title</th>
                          <th>Air date</th>
                          <th>File</th>
                          <th />
                        </tr>
                      </thead>
                      <tbody>
                        {list.map((e) => (
                          <tr key={e.id}>
                            <td>
                              <input type="checkbox" checked={e.monitored} onChange={() => toggleEpisode(e)} aria-label={`Monitor ${e.title}`} />
                            </td>
                            <td className="num">{e.episode_number}</td>
                            <td style={{ wordBreak: "break-word" }}>
                              {e.title}
                              {e.absolute_episode_number != null && <span className="muted small"> · abs {e.absolute_episode_number}</span>}
                              {e.finale_type && <span className="badge"> {e.finale_type} finale</span>}
                            </td>
                            <td className="small muted">{e.air_date ?? "TBA"}</td>
                            <td className="small">
                              {e.file ? (
                                <span>
                                  <span className="badge good">{e.file.quality}</span> {formatSize(e.file.size)}{" "}
                                  <button onClick={() => deleteFile(e)} aria-label="Delete file">
                                    ×
                                  </button>
                                </span>
                              ) : e.monitored && e.air_date && e.air_date <= new Date().toISOString().slice(0, 10) ? (
                                <span className="badge warn">Missing</span>
                              ) : (
                                <span className="muted">—</span>
                              )}
                            </td>
                            <td className="row end">
                              <button onClick={() => searchEpisode(e)} aria-label={`Search for ${e.title}`} title="Search">
                                Search
                              </button>
                              <button onClick={() => setInteractive(interactive?.episode === e.id ? null : { episode: e.id })} aria-label={`Interactive search for ${e.title}`}>
                                Manual
                              </button>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </section>
            );
          })}
          <h2>History</h2>
          <LoadState loading={history.loading} error={history.error} hasData={!!history.data} />
          {history.data && history.data.length === 0 && <p className="muted">Nothing has happened yet.</p>}
          {history.data && history.data.length > 0 && (
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Event</th>
                    <th>Episode</th>
                    <th>Release</th>
                    <th>Quality</th>
                    <th>When</th>
                  </tr>
                </thead>
                <tbody>
                  {history.data.map((h) => (
                    <tr key={h.id}>
                      <td>
                        <span className={`badge ${h.event === "downloadFailed" ? "bad" : h.event === "grabbed" ? "accent" : "good"}`}>{EVENT_LABEL[h.event] ?? h.event}</span>
                      </td>
                      <td className="small">
                        {h.season_number}x{String(h.episode_number ?? 0).padStart(2, "0")} {h.episode_title}
                      </td>
                      <td style={{ wordBreak: "break-word" }}>
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
          <p className="muted small" style={{ marginTop: "1rem" }}>
            <Link href="/series/">Back to series</Link>
          </p>
        </>
      )}
    </>
  );
}

function SeriesSettings({ series, profiles, onSaved }: { series: Series; profiles: QualityProfile[]; onSaved: () => void }) {
  const notify = useToast();
  const [type, setType] = useState(series.series_type);
  const [profile, setProfile] = useState(series.quality_profile_id);
  const [seasonFolder, setSeasonFolder] = useState(series.season_folder);
  const [newItems, setNewItems] = useState(series.monitor_new_items);
  const [monitor, setMonitor] = useState("");
  const [tags, setTags] = useState(series.tags);
  const allTags = useLoad(() => get<Tag[]>("/tag"));
  const [busy, setBusy] = useState(false);

  async function save() {
    setBusy(true);
    try {
      await put(`/series/${series.id}`, { series_type: type, quality_profile_id: profile, season_folder: seasonFolder, monitor_new_items: newItems, tags });
      if (monitor) await post(`/series/${series.id}/monitor`, { monitor });
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
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Series type</span>
          <select value={type} onChange={(e) => setType(e.target.value as Series["series_type"])}>
            <option value="standard">Standard</option>
            <option value="daily">Daily</option>
            <option value="anime">Anime</option>
          </select>
        </label>
        <label className="field">
          <span>New episodes</span>
          <select value={newItems} onChange={(e) => setNewItems(e.target.value as Series["monitor_new_items"])}>
            <option value="all">Monitor</option>
            <option value="none">Don’t monitor</option>
          </select>
        </label>
        <label className="field">
          <span>Change monitoring now</span>
          <select value={monitor} onChange={(e) => setMonitor(e.target.value)}>
            <option value="">Leave as it is</option>
            <option value="all">All episodes</option>
            <option value="future">Future episodes</option>
            <option value="missing">Missing episodes</option>
            <option value="existing">Existing episodes</option>
            <option value="recent">Recent episodes</option>
            <option value="first_season">First season</option>
            <option value="last_season">Last season</option>
            <option value="pilot">Pilot only</option>
            <option value="none">None</option>
          </select>
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={seasonFolder} onChange={(e) => setSeasonFolder(e.target.checked)} /> Season folders
      </label>
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
