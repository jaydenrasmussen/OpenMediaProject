"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { AddMovieForm } from "../components/AddMovieForm";
import { AddSeriesForm } from "../components/AddSeriesForm";
import { Backdrop } from "../components/Hero";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { CastAndMore, Credits } from "../components/TitleExtras";
import { get } from "../lib/api";
import { formatRuntime } from "../lib/format";
import { useLoad } from "../lib/hooks";
import type { BrowseTitle } from "../lib/types";

export default function TitlePage() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <Title />
    </Suspense>
  );
}

function Title() {
  const q = useSearchParams();
  const kind = q.get("kind") === "tv" ? "tv" : "movie";
  const id = Number(q.get("id"));
  const t = useLoad(() => get<BrowseTitle>(`/browse/title/${kind}/${id}`), [kind, id]);
  const [adding, setAdding] = useState(false);
  const d = t.data;
  const library = d?.in_library ? (kind === "tv" ? `/series/show/?id=${d.in_library}` : `/movie/?id=${d.in_library}`) : null;

  return (
    <>
      <LoadState loading={t.loading} error={t.error} hasData={!!d} />
      {d && (
        <>
          <Backdrop images={d.images} />
          <div className="detail over">
            <Poster images={d.images} title={d.title} width={500} />
            <div className="stack">
              <div>
                <h1 style={{ marginBottom: ".25rem" }}>
                  {d.title} <span className="muted">({d.year || "–"})</span>
                </h1>
                {d.tagline && <div className="muted tagline">{d.tagline}</div>}
                <div className="row small muted" style={{ marginTop: ".5rem" }}>
                  {d.vote > 0 && <span>★ {d.vote.toFixed(1)}</span>}
                  {d.runtime ? <span>{formatRuntime(d.runtime)}</span> : null}
                  {d.seasons ? <span>{d.seasons} seasons</span> : null}
                  {d.status && <span className="badge">{d.status}</span>}
                  {d.networks.map((n) => (
                    <span key={n}>{n}</span>
                  ))}
                  {d.genres.slice(0, 4).map((g) => (
                    <span key={g} className="badge">
                      {g}
                    </span>
                  ))}
                  {d.media_servers.length > 0 && <span className="badge good">On {d.media_servers.join(", ")}</span>}
                </div>
              </div>
              {d.overview && <p>{d.overview}</p>}
              <div className="row">
                {library ? (
                  <Link href={library} className="button primary">
                    Open in library
                  </Link>
                ) : (
                  <button className="primary" onClick={() => setAdding((v) => !v)}>
                    {adding ? "Cancel" : kind === "tv" ? "Add series" : "Add movie"}
                  </button>
                )}
                {d.trailer && (
                  <a className="button" href={`https://www.youtube.com/watch?v=${d.trailer}`} target="_blank" rel="noreferrer noopener">
                    Trailer
                  </a>
                )}
              </div>
              {adding && !library && <div className="card">{kind === "tv" ? <AddSeriesForm tmdbId={d.tmdb_id} /> : <AddMovieForm tmdbId={d.tmdb_id} />}</div>}
              <Credits title={d} />
            </div>
          </div>
          <div style={{ marginTop: "2.5rem" }}>
            <CastAndMore title={d} />
          </div>
        </>
      )}
    </>
  );
}
