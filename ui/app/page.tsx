"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { CustomFilterBar } from "./components/CustomFilterBar";
import { LoadState } from "./components/Page";
import { Poster } from "./components/Poster";
import { get } from "./lib/api";
import { useLoad } from "./lib/hooks";
import { useLive } from "./lib/live";
import type { Movie } from "./lib/types";

type Filter = "all" | "wanted" | "downloaded" | "unmonitored";
type Sort = "title" | "year" | "added";
type SavedView = { filter: Filter; sort: Sort };

function badge(m: Movie) {
  if (m.has_file) return <span className="badge good">Downloaded</span>;
  if (!m.monitored) return <span className="badge">Unmonitored</span>;
  return <span className="badge warn">Wanted</span>;
}

export default function Library() {
  const { data, error, loading, reload } = useLoad(() => get<Movie[]>("/movie"));
  useLive(reload, ["history"]);
  const [text, setText] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("title");

  const movies = useMemo(() => {
    const q = text.trim().toLowerCase();
    return (data ?? [])
      .filter((m) => !q || m.title.toLowerCase().includes(q))
      .filter((m) => {
        switch (filter) {
          case "wanted":
            return m.monitored && !m.has_file;
          case "downloaded":
            return m.has_file;
          case "unmonitored":
            return !m.monitored;
          default:
            return true;
        }
      })
      .sort((a, b) => {
        if (sort === "year") return b.year - a.year || a.title.localeCompare(b.title);
        if (sort === "added") return b.added.localeCompare(a.added);
        return a.title.localeCompare(b.title);
      });
  }, [data, text, filter, sort]);

  return (
    <>
      <div className="spread">
        <h1>Movies</h1>
        <div className="row">
          <input
            type="search"
            placeholder="Filter by title"
            value={text}
            onChange={(e) => setText(e.target.value)}
            aria-label="Filter by title"
          />
          <select value={filter} onChange={(e) => setFilter(e.target.value as Filter)} aria-label="Show">
            <option value="all">All</option>
            <option value="wanted">Wanted</option>
            <option value="downloaded">Downloaded</option>
            <option value="unmonitored">Unmonitored</option>
          </select>
          <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort by">
            <option value="title">Title</option>
            <option value="year">Year</option>
            <option value="added">Recently added</option>
          </select>
          <CustomFilterBar<SavedView>
            filterType="movies"
            state={{ filter, sort }}
            onApply={(v) => {
              setFilter(v.filter);
              setSort(v.sort);
            }}
          />
        </div>
      </div>

      <LoadState loading={loading} error={error} hasData={!!data} />

      {data && data.length === 0 && (
        <div className="card">
          <p>Your library is empty.</p>
          <p>
            <Link href="/settings/">Add a root folder</Link>, then{" "}
            <Link href="/add/">add your first movie</Link>.
          </p>
        </div>
      )}
      {data && data.length > 0 && movies.length === 0 && <p className="muted">No movies match.</p>}

      <div className="grid">
        {movies.map((m) => (
          <Link key={m.id} href={`/movie/?id=${m.id}`} className="poster-card">
            <Poster images={m.images} title={m.title} />
            <div className="poster-meta">
              <div className="title" title={m.title}>
                {m.title}
              </div>
              <div className="row small muted" style={{ justifyContent: "space-between" }}>
                <span>{m.year || "–"}</span>
                {badge(m)}
              </div>
            </div>
          </Link>
        ))}
      </div>
    </>
  );
}
