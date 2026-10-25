"use client";

import Link from "next/link";
import { useCallback, useEffect, useRef, useState } from "react";
import { get } from "../lib/api";
import { errorMessage } from "../lib/hooks";
import type { BrowseItem, Paged } from "../lib/types";
import { Poster } from "./Poster";

/** Where an item's own page is. */
export function itemHref(i: BrowseItem): string {
  return i.kind === "person" ? `/person/?id=${i.tmdb_id}` : `/title/?kind=${i.kind}&id=${i.tmdb_id}`;
}

/** A poster that opens the item's page, with what you did or hold of it underneath. */
export function BrowseCard({ item }: { item: BrowseItem }) {
  return (
    <Link href={itemHref(item)} className={`poster-card ${item.kind === "person" ? "person" : ""}`}>
      <Poster images={item.images} title={item.title} />
      <div className="poster-meta">
        <div className="title" title={item.title}>
          {item.title}
        </div>
        <div className="row small muted" style={{ justifyContent: "space-between", flexWrap: "nowrap" }}>
          <span className="clip">{item.kind === "person" ? (item.role ?? item.department ?? "") : (item.role ?? (item.year || "–"))}</span>
          {item.in_library ? <span className="badge accent">In library</span> : item.media_servers?.length ? <span className="badge good">On a server</span> : null}
        </div>
      </div>
    </Link>
  );
}

/** One row of posters that scrolls sideways. */
export function Shelf({ title, items, more }: { title: string; items: BrowseItem[]; more?: string }) {
  if (items.length === 0) return null;
  return (
    <section className="shelf">
      <div className="spread">
        <h2 className="shelf-title">{title}</h2>
        {more && (
          <Link href={more} className="muted small">
            See all
          </Link>
        )}
      </div>
      <div className="shelf-row">
        {items.map((i) => (
          <BrowseCard key={`${i.kind}${i.tmdb_id}`} item={i} />
        ))}
      </div>
    </section>
  );
}

/** A grid that fetches page after page: more arrive when the button is pressed or the end comes into view. */
export function PagedGrid({ path, empty = "Nothing found." }: { path: string | null; empty?: string }) {
  const [items, setItems] = useState<BrowseItem[]>([]);
  const [page, setPage] = useState(0);
  const [pages, setPages] = useState(1);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const end = useRef<HTMLDivElement>(null);

  const load = useCallback(
    async (next: number) => {
      if (!path) return;
      setBusy(true);
      setError(null);
      try {
        const r = await get<Paged<BrowseItem>>(`${path}${path.includes("?") ? "&" : "?"}page=${next}`);
        setItems((old) => (next === 1 ? r.results : [...old, ...r.results.filter((x) => !old.some((o) => o.kind === x.kind && o.tmdb_id === x.tmdb_id))]));
        setPage(r.page);
        setPages(r.total_pages);
        setTotal(r.total_results);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [path],
  );

  useEffect(() => {
    setItems([]);
    setPage(0);
    setPages(1);
    if (path) load(1);
  }, [path, load]);

  const more = page > 0 && page < pages;
  useEffect(() => {
    const el = end.current;
    if (!el || !more || busy || typeof IntersectionObserver === "undefined") return;
    const watch = new IntersectionObserver((e) => e[0].isIntersecting && load(page + 1), { rootMargin: "600px" });
    watch.observe(el);
    return () => watch.disconnect();
  }, [more, busy, page, load]);

  return (
    <>
      {error && <p className="error">{error}</p>}
      {page > 0 && items.length === 0 && !busy && <p className="muted">{empty}</p>}
      <div className="grid">
        {items.map((i) => (
          <BrowseCard key={`${i.kind}${i.tmdb_id}`} item={i} />
        ))}
      </div>
      <div ref={end} className="row end-of-list">
        {busy && <span className="muted">Loading…</span>}
        {more && !busy && (
          <button onClick={() => load(page + 1)}>
            Show more{total ? ` (${items.length} of ${total})` : ""}
          </button>
        )}
        {page > 0 && !more && items.length > 0 && <span className="muted small">That is everything{total ? `: ${total}` : ""}.</span>}
      </div>
    </>
  );
}
