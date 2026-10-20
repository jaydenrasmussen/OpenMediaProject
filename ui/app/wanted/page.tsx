"use client";

import Link from "next/link";
import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { get, post } from "../lib/api";
import { formatDate } from "../lib/format";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Movie, WantedEpisode } from "../lib/types";

type Tab = "missing" | "cutoff";

export default function Wanted() {
  const [kind, setKind] = useState<"movies" | "episodes">("movies");
  return (
    <>
      <h1>Wanted</h1>
      <div className="tabs" role="tablist" aria-label="Kind">
        <button role="tab" className={kind === "movies" ? "active" : ""} onClick={() => setKind("movies")}>
          Movies
        </button>
        <button role="tab" className={kind === "episodes" ? "active" : ""} onClick={() => setKind("episodes")}>
          Episodes
        </button>
      </div>
      {kind === "movies" ? <WantedMovies /> : <WantedEpisodes />}
    </>
  );
}

function WantedEpisodes() {
  const notify = useToast();
  const [tab, setTab] = useState<Tab>("missing");
  const list = useLoad(() => get<{ total: number; items: WantedEpisode[] }>(`/wanted/episodes/${tab}?limit=200`), [tab]);
  const [searching, setSearching] = useState<Set<number>>(new Set());

  async function search(ids: number[]) {
    for (const id of ids) {
      setSearching((s) => new Set(s).add(id));
      try {
        await post("/command", { name: "EpisodeSearch", episode_id: id, wait: true });
      } catch (err) {
        notify(errorMessage(err), "bad");
        break;
      } finally {
        setSearching((s) => {
          const next = new Set(s);
          next.delete(id);
          return next;
        });
      }
    }
    list.reload();
  }

  const items = list.data?.items ?? [];
  return (
    <>
      <div className="tabs" role="tablist">
        <button role="tab" className={tab === "missing" ? "active" : ""} onClick={() => setTab("missing")}>
          Missing
        </button>
        <button role="tab" className={tab === "cutoff" ? "active" : ""} onClick={() => setTab("cutoff")}>
          Cutoff unmet
        </button>
      </div>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data && items.length === 0 && <p className="muted">{tab === "missing" ? "Every monitored episode that has aired has a file." : "Every file meets its profile’s cutoff."}</p>}
      {items.length > 0 && (
        <>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Series</th>
                  <th>Episode</th>
                  <th>{tab === "missing" ? "Aired" : "Current quality"}</th>
                  <th>Last searched</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {items.map((e) => (
                  <tr key={e.id}>
                    <td>
                      <Link href={`/series/show/?id=${e.series_id}`}>{e.series_title}</Link>
                    </td>
                    <td>
                      {e.season_number}x{String(e.episode_number).padStart(2, "0")} <span className="muted">{e.title}</span>
                    </td>
                    <td className="small muted">{tab === "missing" ? (e.air_date ?? "—") : (e.quality ?? "")}</td>
                    <td className="small muted">{e.last_search_time ? formatDate(e.last_search_time) : "never"}</td>
                    <td>
                      <button disabled={searching.has(e.id)} onClick={() => search([e.id])}>
                        {searching.has(e.id) ? "Searching…" : "Search"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="row end" style={{ marginTop: "1rem" }}>
            <span className="muted small">{list.data!.total > items.length ? `Showing ${items.length} of ${list.data!.total}. ` : ""}</span>
            <button className="primary" disabled={searching.size > 0} onClick={() => search(items.map((e) => e.id))}>
              Search all {items.length}
            </button>
          </div>
        </>
      )}
    </>
  );
}

function WantedMovies() {
  const notify = useToast();
  const [tab, setTab] = useState<Tab>("missing");
  const list = useLoad(() => get<Movie[]>(`/wanted/${tab}`), [tab]);
  const [searching, setSearching] = useState<Set<number>>(new Set());

  async function search(ids: number[]) {
    for (const id of ids) {
      setSearching((s) => new Set(s).add(id));
      try {
        await post("/command", { name: "MoviesSearch", movie_id: id, wait: true });
      } catch (err) {
        notify(errorMessage(err), "bad");
        break;
      } finally {
        setSearching((s) => {
          const next = new Set(s);
          next.delete(id);
          return next;
        });
      }
    }
    list.reload();
  }

  const movies = list.data ?? [];
  return (
    <>
      <div className="tabs" role="tablist">
        <button role="tab" className={tab === "missing" ? "active" : ""} onClick={() => setTab("missing")}>
          Missing
        </button>
        <button role="tab" className={tab === "cutoff" ? "active" : ""} onClick={() => setTab("cutoff")}>
          Cutoff unmet
        </button>
      </div>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data && movies.length === 0 && (
        <p className="muted">{tab === "missing" ? "Every monitored movie has a file." : "Every file meets its profile’s cutoff."}</p>
      )}
      {movies.length > 0 && (
        <>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Movie</th>
                  <th>{tab === "missing" ? "Status" : "Current quality"}</th>
                  <th>Last searched</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {movies.map((m) => (
                  <tr key={m.id}>
                    <td>
                      <Link href={`/movie/?id=${m.id}`}>{m.title}</Link> <span className="muted">({m.year})</span>
                    </td>
                    <td>{tab === "missing" ? m.status : (m.file?.quality ?? "")}</td>
                    <td className="small muted">{m.last_search_time ? formatDate(m.last_search_time) : "never"}</td>
                    <td>
                      <button disabled={searching.has(m.id)} onClick={() => search([m.id])}>
                        {searching.has(m.id) ? "Searching…" : "Search"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="row end" style={{ marginTop: "1rem" }}>
            <button className="primary" disabled={searching.size > 0} onClick={() => search(movies.map((m) => m.id))}>
              Search all {movies.length}
            </button>
          </div>
        </>
      )}
    </>
  );
}
