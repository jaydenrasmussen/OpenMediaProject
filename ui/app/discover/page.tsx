"use client";

import Link from "next/link";
import { useState } from "react";
import { PagedGrid, Shelf, itemHref } from "../components/Browse";
import { Hero } from "../components/Hero";
import { LoadState } from "../components/Page";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { BrowseItem, Shelf as ShelfData } from "../lib/types";

const KINDS = [
  ["multi", "Everything"],
  ["movie", "Movies"],
  ["tv", "Series"],
  ["person", "People"],
] as const;

export default function Discover() {
  const home = useLoad(() => get<{ hero: BrowseItem | null; rows: ShelfData[] }>("/browse/home"));
  const [text, setText] = useState("");
  const [term, setTerm] = useState("");
  const [kind, setKind] = useState<string>("multi");
  const hero = home.data?.hero;

  return (
    <>
      {hero && !term && (
        <Hero
          images={hero.images}
          eyebrow={hero.kind === "tv" ? "Trending series" : "Trending movie"}
          title={hero.title}
          meta={[hero.year || null, hero.vote ? `★ ${hero.vote.toFixed(1)}` : null].filter(Boolean).join("  ·  ")}
          overview={hero.overview}
        >
          <Link href={itemHref(hero)} className="button primary">
            {hero.in_library ? "In your library" : "See details"}
          </Link>
        </Hero>
      )}
      <div className="spread">
        <h1 className="shelf-title">Discover</h1>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            setTerm(text.trim());
          }}
        >
          <input
            type="search"
            className="discover-search"
            placeholder="Search movies, series and people"
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              if (!e.target.value.trim()) setTerm("");
            }}
            aria-label="Search TMDB"
          />
          <button className="primary">Search</button>
        </form>
      </div>
      {term && (
        <>
          <div className="counts" role="group" aria-label="Search in">
            {KINDS.map(([id, label]) => (
              <button key={id} className={kind === id ? "on" : ""} onClick={() => setKind(id)}>
                <span>{label}</span>
              </button>
            ))}
          </div>
          <PagedGrid path={`/browse/search?kind=${kind}&term=${encodeURIComponent(term)}`} empty={`Nothing found for “${term}”.`} />
        </>
      )}
      {!term && (
        <>
          <LoadState loading={home.loading} error={home.error} hasData={!!home.data} />
          {home.data?.rows.map((r) => (
            <Shelf key={r.id} title={r.title} items={r.items} more={`/discover/list/?list=${r.id}`} />
          ))}
        </>
      )}
    </>
  );
}
