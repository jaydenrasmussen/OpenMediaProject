"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { LoadState } from "../components/Page";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { Movie } from "../lib/types";

/** The iCal address for calendar apps, with the API key they need. */
function FeedLink() {
  const key = useLoad(() => get<{ api_key: string }>("/auth/apikey"));
  if (!key.data) return null;
  const url = `${window.location.origin}/api/v1/calendar/feed.ics?apikey=${key.data.api_key}`;
  return (
    <details className="small" style={{ marginBottom: "1rem" }}>
      <summary>Subscribe in a calendar app</summary>
      <p className="muted">Add this address as a calendar subscription. It contains your API key, so keep it private.</p>
      <input type="text" readOnly value={url} onFocus={(e) => e.target.select()} style={{ width: "100%" }} aria-label="Calendar feed address" />
    </details>
  );
}

const KINDS = [
  { key: "in_cinemas", label: "In cinemas" },
  { key: "digital_release", label: "Digital" },
  { key: "physical_release", label: "Physical" },
] as const;

interface Entry {
  day: string; // YYYY-MM-DD
  movie: Movie;
  kind: string;
}

export default function Calendar() {
  const movies = useLoad(() => get<Movie[]>("/movie"));
  const [cursor, setCursor] = useState(() => {
    const now = new Date();
    return { year: now.getFullYear(), month: now.getMonth() };
  });
  const [onlyMonitored, setOnlyMonitored] = useState(true);

  const prefix = `${cursor.year}-${String(cursor.month + 1).padStart(2, "0")}`;
  const days = useMemo(() => {
    const entries: Entry[] = [];
    for (const m of movies.data ?? []) {
      if (onlyMonitored && !m.monitored) continue;
      for (const k of KINDS) {
        const date = m[k.key];
        if (date?.startsWith(prefix)) entries.push({ day: date.slice(0, 10), movie: m, kind: k.label });
      }
    }
    entries.sort((a, b) => a.day.localeCompare(b.day) || a.movie.title.localeCompare(b.movie.title));
    const grouped = new Map<string, Entry[]>();
    for (const e of entries) grouped.set(e.day, [...(grouped.get(e.day) ?? []), e]);
    return [...grouped.entries()];
  }, [movies.data, prefix, onlyMonitored]);

  const shift = (d: number) => {
    const next = new Date(cursor.year, cursor.month + d, 1);
    setCursor({ year: next.getFullYear(), month: next.getMonth() });
  };
  const title = new Date(cursor.year, cursor.month, 1).toLocaleDateString(undefined, { month: "long", year: "numeric" });

  return (
    <>
      <div className="spread">
        <h1>Calendar</h1>
        <label className="check">
          <input type="checkbox" checked={onlyMonitored} onChange={(e) => setOnlyMonitored(e.target.checked)} /> Monitored only
        </label>
      </div>
      <div className="row" style={{ marginBottom: "1rem" }}>
        <button onClick={() => shift(-1)} aria-label="Previous month">
          ←
        </button>
        <strong style={{ minWidth: "10rem", textAlign: "center" }}>{title}</strong>
        <button onClick={() => shift(1)} aria-label="Next month">
          →
        </button>
      </div>
      <LoadState loading={movies.loading} error={movies.error} hasData={!!movies.data} />
      <FeedLink />
      {movies.data && days.length === 0 && <p className="muted">No releases this month.</p>}
      <div className="stack">
        {days.map(([day, entries]) => (
          <section key={day} className="card">
            <strong>{new Date(`${day}T12:00:00`).toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}</strong>
            {entries.map((e) => (
              <div key={`${e.movie.id}-${e.kind}`} className="spread" style={{ marginTop: ".4rem" }}>
                <span>
                  <Link href={`/movie/?id=${e.movie.id}`}>{e.movie.title}</Link> <span className="muted">({e.movie.year})</span>
                </span>
                <span className="row">
                  <span className="badge">{e.kind}</span>
                  {e.movie.has_file ? <span className="badge good">Downloaded</span> : e.movie.monitored ? <span className="badge warn">Wanted</span> : <span className="badge">Unmonitored</span>}
                </span>
              </div>
            ))}
          </section>
        ))}
      </div>
    </>
  );
}
