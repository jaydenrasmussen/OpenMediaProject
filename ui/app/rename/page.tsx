"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { EpisodeRenamePreview } from "../lib/types";

interface Preview {
  movie_id: number;
  title: string;
  existing: string;
  new: string;
}

export default function RenamePage() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <Rename />
    </Suspense>
  );
}

function Rename() {
  const notify = useToast();
  const params = useSearchParams();
  const movieId = params.get("id");
  const seriesId = params.get("series_id");
  // A link from one movie or one series shows only that; otherwise everything.
  const wantMovies = !seriesId;
  const wantEpisodes = !movieId;
  const previews = useLoad(
    async () => (wantMovies ? await get<Preview[]>(movieId ? `/rename?movie_id=${movieId}` : "/rename") : []),
    [movieId, seriesId],
  );
  const episodes = useLoad(
    async () => (wantEpisodes ? await get<EpisodeRenamePreview[]>(seriesId ? `/rename/episodes?series_id=${seriesId}` : "/rename/episodes") : []),
    [movieId, seriesId],
  );
  const [busy, setBusy] = useState(false);

  async function apply() {
    setBusy(true);
    try {
      if (wantMovies && previews.data?.length) {
        await post("/command", movieId ? { name: "RenameMovie", movie_id: Number(movieId), wait: true } : { name: "RenameMovies", wait: true });
      }
      if (wantEpisodes && episodes.data?.length) {
        await post("/command", seriesId ? { name: "RenameSeries", series_id: Number(seriesId), wait: true } : { name: "RenameSeries", wait: true });
      }
      notify("Renamed.", "good");
      previews.reload();
      episodes.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1>Rename files</h1>
      <p className="muted small">
        Files are renamed to match the naming format in <Link href="/settings/">Settings</Link>. Renaming has to be switched on there first.
      </p>
      <LoadState loading={previews.loading || episodes.loading} error={previews.error ?? episodes.error} hasData={!!previews.data && !!episodes.data} />
      {previews.data && episodes.data && previews.data.length + episodes.data.length === 0 && <p className="muted">Every file already has the right name.</p>}
      {previews.data && episodes.data && previews.data.length + episodes.data.length > 0 && (
        <>
          <div className="stack">
            {previews.data.map((p) => (
              <div key={p.movie_id} className="card">
                <Link href={`/movie/?id=${p.movie_id}`}>
                  <strong>{p.title}</strong>
                </Link>
                <div className="small muted" style={{ wordBreak: "break-all" }}>
                  {p.existing}
                </div>
                <div className="small" style={{ wordBreak: "break-all" }}>
                  → {p.new}
                </div>
              </div>
            ))}
            {episodes.data.map((p) => (
              <div key={p.episode_file_id} className="card">
                <Link href={`/series/show/?id=${p.series_id}`}>
                  <strong>{p.series_title}</strong>
                </Link>
                <div className="small muted" style={{ wordBreak: "break-all" }}>
                  {p.existing}
                </div>
                <div className="small" style={{ wordBreak: "break-all" }}>
                  → {p.new}
                </div>
              </div>
            ))}
          </div>
          <div className="row end" style={{ marginTop: "1rem" }}>
            <button className="primary" onClick={apply} disabled={busy}>
              {busy ? "Renaming…" : `Rename ${previews.data.length + episodes.data.length} file${previews.data.length + episodes.data.length === 1 ? "" : "s"}`}
            </button>
          </div>
        </>
      )}
    </>
  );
}
