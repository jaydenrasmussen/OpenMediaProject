"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { PagedGrid } from "../../components/Browse";

const TITLES: Record<string, string> = {
  trending_movie: "Trending movies",
  popular_movie: "Popular movies",
  top_movie: "Top rated movies",
  upcoming_movie: "Coming soon",
  trending_tv: "Trending series",
  popular_tv: "Popular series",
  top_tv: "Top rated series",
};

export default function ListPage() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <List />
    </Suspense>
  );
}

function List() {
  const list = useSearchParams().get("list") ?? "";
  return (
    <>
      <p className="small">
        <Link href="/discover/">← Discover</Link>
      </p>
      <h1 className="shelf-title" style={{ marginBottom: "1.25rem" }}>
        {TITLES[list] ?? "List"}
      </h1>
      <PagedGrid path={TITLES[list] ? `/browse/list?list=${list}` : null} />
    </>
  );
}
