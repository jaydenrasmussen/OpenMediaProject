"use client";

import Link from "next/link";
import { useState } from "react";
import { get } from "../lib/api";
import { errorMessage } from "../lib/hooks";
import type { BrowseItem, LookupResult, Paged, SeriesLookupResult } from "../lib/types";
import { AddMovieForm } from "./AddMovieForm";
import { AddSeriesForm } from "./AddSeriesForm";
import { itemHref } from "./Browse";
import { Poster } from "./Poster";

type Row = Pick<BrowseItem, "tmdb_id" | "title" | "year" | "overview" | "images" | "in_library" | "media_servers" | "kind">;

/** Search to add a movie or series: a page at a time, each hit opening its own page. */
export function AddSearch({ kind, placeholder }: { kind: "movie" | "tv"; placeholder: string }) {
  const [term, setTerm] = useState("");
  const [asked, setAsked] = useState("");
  const [rows, setRows] = useState<Row[] | null>(null);
  const [page, setPage] = useState(1);
  const [pages, setPages] = useState(1);
  const [total, setTotal] = useState(0);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<number | null>(null);

  // An id lookup (tmdb:603, imdb:tt…) is a single answer; words are paged.
  const byId = (t: string) => /^(tmdb|imdb|tvdb):/i.test(t);

  async function fetchPage(t: string, n: number) {
    if (byId(t)) {
      const path = kind === "tv" ? "/series/lookup" : "/movie/lookup";
      const found = await get<(LookupResult | SeriesLookupResult)[]>(`${path}?term=${encodeURIComponent(t)}`);
      return { rows: found.map((f) => ({ ...f, kind }) as Row), page: 1, pages: 1, total: found.length };
    }
    const r = await get<Paged<BrowseItem>>(`/browse/search?kind=${kind}&term=${encodeURIComponent(t)}&page=${n}`);
    return { rows: r.results, page: r.page, pages: r.total_pages, total: r.total_results };
  }

  async function run(t: string, n: number, append: boolean) {
    setSearching(true);
    setError(null);
    try {
      const r = await fetchPage(t, n);
      setRows((old) => (append && old ? [...old, ...r.rows.filter((x) => !old.some((o) => o.tmdb_id === x.tmdb_id))] : r.rows));
      setPage(r.page);
      setPages(r.pages);
      setTotal(r.total);
    } catch (err) {
      if (!append) setRows(null);
      setError(errorMessage(err));
    } finally {
      setSearching(false);
    }
  }

  function search(e: React.FormEvent) {
    e.preventDefault();
    const t = term.trim();
    if (!t) return;
    setAsked(t);
    setOpened(null);
    run(t, 1, false);
  }

  const libraryHref = (id: number) => (kind === "tv" ? `/series/show/?id=${id}` : `/movie/?id=${id}`);

  return (
    <>
      <form className="row" onSubmit={search}>
        <input
          type="search"
          style={{ flex: 1, minWidth: 220 }}
          placeholder={placeholder}
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          autoFocus
          aria-label={kind === "tv" ? "Search for a series" : "Search for a movie"}
        />
        <button className="primary" disabled={searching}>
          {searching && page === 1 ? "Searching…" : "Search"}
        </button>
      </form>
      {error && <p className="error">{error}</p>}
      {rows && rows.length === 0 && <p className="muted">Nothing found.</p>}
      {rows && rows.length > 0 && !byId(asked) && (
        <p className="muted small" style={{ marginTop: ".75rem" }}>
          Showing {rows.length} of {total}
        </p>
      )}

      <div className="stack" style={{ marginTop: "1rem" }}>
        {rows?.map((r) => (
          <div key={r.tmdb_id} className="card">
            <div className="row" style={{ alignItems: "flex-start", flexWrap: "nowrap" }}>
              <Link href={itemHref({ ...(r as BrowseItem), kind })} style={{ width: 90, flexShrink: 0 }}>
                <Poster images={r.images} title={r.title} width={154} />
              </Link>
              <div className="stack" style={{ flex: 1, minWidth: 0 }}>
                <div className="spread">
                  <strong>
                    <Link href={itemHref({ ...(r as BrowseItem), kind })}>{r.title}</Link>{" "}
                    {r.year ? <span className="muted">({r.year})</span> : null}{" "}
                    {r.media_servers?.length > 0 && <span className="badge good">On {r.media_servers.join(", ")}</span>}
                  </strong>
                  {r.in_library ? (
                    <Link href={libraryHref(r.in_library)}>In library</Link>
                  ) : (
                    <button onClick={() => setOpened(opened === r.tmdb_id ? null : r.tmdb_id)}>{opened === r.tmdb_id ? "Cancel" : "Add"}</button>
                  )}
                </div>
                <p className="muted small" style={{ maxHeight: "4.5em", overflow: "hidden" }}>
                  {r.overview}
                </p>
                {opened === r.tmdb_id && (kind === "tv" ? <AddSeriesForm tmdbId={r.tmdb_id} /> : <AddMovieForm tmdbId={r.tmdb_id} />)}
              </div>
            </div>
          </div>
        ))}
      </div>
      {rows && page < pages && (
        <div className="row end-of-list">
          <button onClick={() => run(asked, page + 1, true)} disabled={searching}>
            {searching ? "Loading…" : "Show more"}
          </button>
        </div>
      )}
    </>
  );
}
