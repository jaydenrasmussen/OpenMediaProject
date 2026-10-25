"use client";

import Link from "next/link";
import { get, imageSrc } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { BrowseTitle, Credit } from "../lib/types";
import { Shelf } from "./Browse";

function Names({ label, people }: { label: string; people: Credit[] }) {
  if (people.length === 0) return null;
  return (
    <div className="credit-line">
      <span className="muted">{label}</span>{" "}
      {people.slice(0, 6).map((p, i) => (
        <span key={p.id}>
          {i > 0 && ", "}
          <Link href={`/person/?id=${p.id}`}>{p.name}</Link>
        </span>
      ))}
    </div>
  );
}

/** Who made it and who is in it, each one a way to see more of their work. */
export function Credits({ title }: { title: BrowseTitle }) {
  return (
    <>
      <div className="stack credit-lines">
        <Names label="Directed by" people={title.directors} />
        <Names label="Written by" people={title.writers} />
        <Names label="Produced by" people={title.producers} />
      </div>
      {title.companies.length > 0 && (
        <div className="row companies">
          {title.companies.map((c) => (
            <Link key={c.id} href={`/company/?id=${c.id}`} className="company-chip" title={c.country ?? ""}>
              {c.logo ? (
                // eslint-disable-next-line @next/next/no-img-element
                <img src={imageSrc(c.logo.replace("/t/p/original/", "/t/p/w185/"))} alt={c.name ?? ""} />
              ) : (
                <span>{c.name}</span>
              )}
            </Link>
          ))}
        </div>
      )}
    </>
  );
}

export function CastAndMore({ title }: { title: BrowseTitle }) {
  return (
    <>
      <Shelf title="Cast" items={title.cast} />
      <Shelf title="You might also like" items={title.recommended} />
    </>
  );
}

/** For a library page: loads the title from TMDB and shows its credits, cast and companies. */
export function TitleExtras({ kind, tmdbId }: { kind: "movie" | "tv"; tmdbId: number }) {
  const t = useLoad(() => get<BrowseTitle>(`/browse/title/${kind}/${tmdbId}`), [kind, tmdbId]);
  if (!t.data) return null;
  return (
    <section className="stack" style={{ marginTop: "2rem" }}>
      <Credits title={t.data} />
      <CastAndMore title={t.data} />
    </section>
  );
}
