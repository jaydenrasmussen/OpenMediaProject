"use client";

import { useEffect, useState } from "react";
import { LoadState } from "./Page";
import { useToast } from "./toast";
import { get, post } from "../lib/api";
import { formatSize } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Episode, Movie, Series } from "../lib/types";

interface Candidate {
  path: string;
  name: string;
  relative_path: string;
  size: number;
  quality: string | null;
  movie: { id: number; title: string; year: number } | null;
  series: { id: number; title: string } | null;
  episodes: { id: number; season_number: number; episode_number: number; title: string }[];
}

interface Choice {
  include: boolean;
  kind: "movie" | "series";
  movieId: number | null;
  seriesId: number | null;
  episodeIds: number[];
}

function initial(c: Candidate): Choice {
  if (c.series && c.episodes.length > 0) return { include: true, kind: "series", movieId: null, seriesId: c.series.id, episodeIds: c.episodes.map((e) => e.id) };
  if (c.movie) return { include: true, kind: "movie", movieId: c.movie.id, seriesId: null, episodeIds: [] };
  return { include: false, kind: "movie", movieId: null, seriesId: null, episodeIds: [] };
}

/** Choose what each file in a download is, then import them. */
export function ManualImport({ downloadId, onClose }: { downloadId: string; onClose: (imported: boolean) => void }) {
  const notify = useToast();
  const files = useLoad(() => get<Candidate[]>(`/manualimport?download_id=${encodeURIComponent(downloadId)}`), [downloadId]);
  const movies = useLoad(() => get<Movie[]>("/movie"));
  const series = useLoad(() => get<Series[]>("/series"));
  const [choices, setChoices] = useState<Choice[]>([]);
  const [busy, setBusy] = useState(false);
  const [problems, setProblems] = useState<Record<string, string>>({});

  useEffect(() => {
    if (files.data) setChoices(files.data.map(initial));
  }, [files.data]);

  const set = (i: number, patch: Partial<Choice>) => setChoices((all) => all.map((c, n) => (n === i ? { ...c, ...patch } : c)));
  const ready = (c: Choice) => c.include && (c.kind === "movie" ? c.movieId != null : c.seriesId != null && c.episodeIds.length > 0);
  const count = choices.filter(ready).length;

  async function run() {
    const items = (files.data ?? [])
      .map((f, i) => ({ f, c: choices[i] }))
      .filter(({ c }) => c && ready(c))
      .map(({ f, c }) => (c.kind === "movie" ? { path: f.path, movie_id: c.movieId } : { path: f.path, series_id: c.seriesId, episode_ids: c.episodeIds }));
    setBusy(true);
    try {
      const res = await post<{ results: { path: string; ok: boolean; error?: string }[] }>("/manualimport", { items, download_id: downloadId });
      const failed = res.results.filter((r) => !r.ok);
      setProblems(Object.fromEntries(failed.map((r) => [r.path, r.error ?? "failed"])));
      notify(`Imported ${res.results.length - failed.length} of ${res.results.length}.`, failed.length ? "info" : "good");
      if (failed.length === 0) onClose(true);
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card stack" style={{ marginTop: "1rem" }}>
      <div className="spread">
        <strong>Manual import</strong>
        <button onClick={() => onClose(false)}>Close</button>
      </div>
      <LoadState loading={files.loading} error={files.error} hasData={!!files.data} />
      {files.data && files.data.length === 0 && <p className="muted">No video files were found in this download.</p>}
      {files.data?.map((f, i) => {
        const c = choices[i];
        if (!c) return null;
        return (
          <div key={f.path} className="card stack">
            <label className="check">
              <input type="checkbox" checked={c.include} onChange={(e) => set(i, { include: e.target.checked })} /> <strong style={{ wordBreak: "break-word" }}>{f.relative_path}</strong>
            </label>
            <div className="small muted">
              {formatSize(f.size)} {f.quality && <>· {f.quality}</>}
            </div>
            {problems[f.path] && <div className="small error">{problems[f.path]}</div>}
            <div className="row">
              <select value={c.kind} onChange={(e) => set(i, { kind: e.target.value as Choice["kind"], include: true })} aria-label="Kind">
                <option value="movie">Movie</option>
                <option value="series">Series</option>
              </select>
              {c.kind === "movie" ? (
                <select value={c.movieId ?? ""} onChange={(e) => set(i, { movieId: e.target.value ? Number(e.target.value) : null })} aria-label="Movie">
                  <option value="">Choose a movie…</option>
                  {movies.data?.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.title} ({m.year})
                    </option>
                  ))}
                </select>
              ) : (
                <>
                  <select value={c.seriesId ?? ""} onChange={(e) => set(i, { seriesId: e.target.value ? Number(e.target.value) : null, episodeIds: [] })} aria-label="Series">
                    <option value="">Choose a series…</option>
                    {series.data?.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.title}
                      </option>
                    ))}
                  </select>
                  {c.seriesId != null && <EpisodePicker seriesId={c.seriesId} value={c.episodeIds} onChange={(episodeIds) => set(i, { episodeIds })} />}
                </>
              )}
            </div>
          </div>
        );
      })}
      <div className="row end">
        <button className="primary" onClick={run} disabled={busy || count === 0}>
          {busy ? "Importing…" : `Import ${count} file${count === 1 ? "" : "s"}`}
        </button>
      </div>
    </section>
  );
}

function EpisodePicker({ seriesId, value, onChange }: { seriesId: number; value: number[]; onChange: (v: number[]) => void }) {
  const eps = useLoad(() => get<Episode[]>(`/episode?series_id=${seriesId}`), [seriesId]);
  return (
    <select
      multiple
      size={5}
      aria-label="Episodes"
      value={value.map(String)}
      onChange={(e) => onChange([...e.target.selectedOptions].map((o) => Number(o.value)))}
      style={{ minWidth: 260 }}
    >
      {eps.data?.map((e) => (
        <option key={e.id} value={e.id}>
          {e.season_number}x{String(e.episode_number).padStart(2, "0")} {e.title}
        </option>
      ))}
    </select>
  );
}
