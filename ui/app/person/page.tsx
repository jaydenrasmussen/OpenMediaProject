"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { BrowseCard } from "../components/Browse";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { BrowsePerson } from "../lib/types";

export default function PersonPage() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <Person />
    </Suspense>
  );
}

function Person() {
  const id = Number(useSearchParams().get("id"));
  const p = useLoad(() => get<BrowsePerson>(`/browse/person/${id}`), [id]);
  const [picked, setPicked] = useState<string>("");
  const d = p.data;
  const sections = d ? Object.keys(d.credits) : [];
  // The work they are best known for leads: acting for an actor, else the first section.
  const first = d && sections.includes(d.department ?? "") ? d.department! : (sections[0] ?? "");
  const section = picked && sections.includes(picked) ? picked : first;
  const [more, setMore] = useState(false);
  const works = d?.credits[section] ?? [];
  const shown = more ? works : works.slice(0, 24);

  return (
    <>
      <LoadState loading={p.loading} error={p.error} hasData={!!d} />
      {d && (
        <>
          <div className="detail" style={{ marginTop: "2rem", gridTemplateColumns: "240px minmax(0, 1fr)" }}>
            <div className="person">
              <Poster images={d.images} title={d.title} width={500} />
            </div>
            <div className="stack">
              <div>
                <h1 style={{ marginBottom: ".25rem" }}>{d.title}</h1>
                <div className="row small muted">
                  {d.department && <span className="badge">{d.department}</span>}
                  {d.birthday && (
                    <span>
                      Born {d.birthday}
                      {d.birthplace ? ` in ${d.birthplace}` : ""}
                    </span>
                  )}
                  {d.deathday && <span>Died {d.deathday}</span>}
                </div>
              </div>
              {d.biography && <p className="bio clamp-8">{d.biography}</p>}
              <p className="small muted">
                <Link href="/discover/">← Discover</Link>
              </p>
            </div>
          </div>
          {sections.length > 0 && (
            <>
              <div className="counts" role="tablist" style={{ marginTop: "2rem" }}>
                {sections.map((s) => (
                  <button key={s} role="tab" className={s === section ? "on" : ""} onClick={() => { setPicked(s); setMore(false); }}>
                    <span>{s}</span>
                    <strong>{d.credits[s].length}</strong>
                  </button>
                ))}
              </div>
              <div className="grid">
                {shown.map((w) => (
                  <BrowseCard key={`${w.kind}${w.tmdb_id}`} item={w} />
                ))}
              </div>
              {works.length > shown.length && (
                <div className="row end-of-list">
                  <button onClick={() => setMore(true)}>Show all {works.length}</button>
                </div>
              )}
            </>
          )}
        </>
      )}
    </>
  );
}
