"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { Series } from "../lib/types";

type Filter = "all" | "continuing" | "ended" | "missing" | "unmonitored";
type Sort = "title" | "year" | "added" | "next";

function badge(s: Series) {
  if (!s.monitored) return <span className="badge">Unmonitored</span>;
  if (s.missing_episode_count > 0) return <span className="badge warn">{s.missing_episode_count} missing</span>;
  if (s.episode_count > 0 && s.episode_file_count === s.episode_count) return <span className="badge good">Complete</span>;
  return <span className="badge accent">{s.status === "ended" ? "Ended" : "Continuing"}</span>;
}

export default function SeriesLibrary() {
  const { data, error, loading } = useLoad(() => get<Series[]>("/series"));
  const [text, setText] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("title");

  const shows = useMemo(() => {
    const q = text.trim().toLowerCase();
    return (data ?? [])
      .filter((s) => !q || s.title.toLowerCase().includes(q))
      .filter((s) => {
        switch (filter) {
          case "continuing":
            return s.status === "continuing";
          case "ended":
            return s.status === "ended";
          case "missing":
            return s.monitored && s.missing_episode_count > 0;
          case "unmonitored":
            return !s.monitored;
          default:
            return true;
        }
      })
      .sort((a, b) => {
        if (sort === "year") return b.year - a.year || a.title.localeCompare(b.title);
        if (sort === "added") return b.added.localeCompare(a.added);
        if (sort === "next") return (a.next_airing ?? "9").localeCompare(b.next_airing ?? "9") || a.title.localeCompare(b.title);
        return a.title.localeCompare(b.title);
      });
  }, [data, text, filter, sort]);

  return (
    <>
      <div className="spread">
        <h1>Series</h1>
        <div className="row">
          <input type="search" placeholder="Filter by title" value={text} onChange={(e) => setText(e.target.value)} aria-label="Filter by title" />
          <select value={filter} onChange={(e) => setFilter(e.target.value as Filter)} aria-label="Show">
            <option value="all">All</option>
            <option value="continuing">Continuing</option>
            <option value="ended">Ended</option>
            <option value="missing">Missing episodes</option>
            <option value="unmonitored">Unmonitored</option>
          </select>
          <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort by">
            <option value="title">Title</option>
            <option value="year">Year</option>
            <option value="added">Recently added</option>
            <option value="next">Next airing</option>
          </select>
        </div>
      </div>

      <LoadState loading={loading} error={error} hasData={!!data} />
      {data && data.length === 0 && (
        <div className="card">
          <p>No series yet.</p>
          <p>
            <Link href="/settings/">Add a root folder</Link>, then <Link href="/series/add/">add your first series</Link>.
          </p>
        </div>
      )}
      {data && data.length > 0 && shows.length === 0 && <p className="muted">No series match.</p>}

      <div className="grid">
        {shows.map((s) => (
          <Link key={s.id} href={`/series/show/?id=${s.id}`} className="poster-card">
            <Poster images={s.images} title={s.title} />
            <div className="poster-meta">
              <div className="title" title={s.title}>
                {s.title}
              </div>
              <div className="row small muted" style={{ justifyContent: "space-between" }}>
                <span>
                  {s.episode_file_count}/{s.episode_count}
                </span>
                {badge(s)}
              </div>
            </div>
          </Link>
        ))}
      </div>
    </>
  );
}
