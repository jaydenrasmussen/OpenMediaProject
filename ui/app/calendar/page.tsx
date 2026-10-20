"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { LoadState } from "../components/Page";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { Movie, WantedEpisode } from "../lib/types";

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
  key: string;
  link: string;
  title: string;
  year?: number;
  kind: string;
  has_file: boolean;
  monitored: boolean;
}

export default function Calendar() {
  const movies = useLoad(() => get<Movie[]>("/movie"));
  const [cursor, setCursor] = useState(() => {
    const now = new Date();
    return { year: now.getFullYear(), month: now.getMonth() };
  });
  const [onlyMonitored, setOnlyMonitored] = useState(true);

  const prefix = `${cursor.year}-${String(cursor.month + 1).padStart(2, "0")}`;
  const last = new Date(cursor.year, cursor.month + 1, 0).getDate();
  const episodes = useLoad(
    () => get<WantedEpisode[]>(`/calendar/episodes?start=${prefix}-01&end=${prefix}-${String(last).padStart(2, "0")}&unmonitored=${!onlyMonitored}`),
    [prefix, onlyMonitored],
  );
  const days = useMemo(() => {
    const entries: Entry[] = [];
    for (const m of movies.data ?? []) {
      if (onlyMonitored && !m.monitored) continue;
      for (const k of KINDS) {
        const date = m[k.key];
        if (date?.startsWith(prefix))
          entries.push({ day: date.slice(0, 10), key: `m${m.id}-${k.key}`, link: `/movie/?id=${m.id}`, title: m.title, year: m.year, kind: k.label, has_file: m.has_file, monitored: m.monitored });
      }
    }
    for (const e of episodes.data ?? []) {
      if (!e.air_date) continue;
      entries.push({
        day: e.air_date,
        key: `e${e.id}`,
        link: `/series/show/?id=${e.series_id}`,
        title: `${e.series_title} ${e.season_number}x${String(e.episode_number).padStart(2, "0")} ${e.title}`,
        kind: "Episode",
        has_file: e.quality != null,
        monitored: e.monitored,
      });
    }
    entries.sort((a, b) => a.day.localeCompare(b.day) || a.title.localeCompare(b.title));
    const grouped = new Map<string, Entry[]>();
    for (const e of entries) grouped.set(e.day, [...(grouped.get(e.day) ?? []), e]);
    return [...grouped.entries()];
  }, [movies.data, episodes.data, prefix, onlyMonitored]);

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
      <LoadState loading={movies.loading || episodes.loading} error={movies.error ?? episodes.error} hasData={!!movies.data && !!episodes.data} />
      <FeedLink />
      {movies.data && episodes.data && days.length === 0 && <p className="muted">Nothing is due this month.</p>}
      <div className="stack">
        {days.map(([day, entries]) => (
          <section key={day} className="card">
            <strong>{new Date(`${day}T12:00:00`).toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}</strong>
            {entries.map((e) => (
              <div key={e.key} className="spread" style={{ marginTop: ".4rem" }}>
                <span>
                  <Link href={e.link}>{e.title}</Link> {e.year != null && <span className="muted">({e.year})</span>}
                </span>
                <span className="row">
                  <span className="badge">{e.kind}</span>
                  {e.has_file ? <span className="badge good">Downloaded</span> : e.monitored ? <span className="badge warn">Wanted</span> : <span className="badge">Unmonitored</span>}
                </span>
              </div>
            ))}
          </section>
        ))}
      </div>
    </>
  );
}
