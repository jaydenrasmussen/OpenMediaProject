"use client";

import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { PagedGrid } from "../components/Browse";
import { LoadState } from "../components/Page";
import { get, imageSrc } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { BrowseCompany } from "../lib/types";

export default function CompanyPage() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <Company />
    </Suspense>
  );
}

function Company() {
  const id = Number(useSearchParams().get("id"));
  const [kind, setKind] = useState<"movie" | "tv">("movie");
  const info = useLoad(() => get<BrowseCompany>(`/browse/company/${id}?kind=movie&page=1`), [id]);
  const c = info.data?.company;
  return (
    <>
      <LoadState loading={info.loading} error={info.error} hasData={!!c} />
      {c && (
        <>
          <div className="company-head">
            {c.logo && (
              <div className="company-logo">
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img src={imageSrc(c.logo.replace("/t/p/original/", "/t/p/w300/"))} alt="" />
              </div>
            )}
            <div className="stack">
              <h1 style={{ marginBottom: ".25rem" }}>{c.name}</h1>
              <div className="row small muted">
                {c.country && <span className="badge">{c.country}</span>}
                {c.headquarters && <span>{c.headquarters}</span>}
                {c.homepage && (
                  <a href={c.homepage} target="_blank" rel="noreferrer noopener">
                    Website
                  </a>
                )}
              </div>
              {c.description && <p className="clamp-3">{c.description}</p>}
            </div>
          </div>
          <div className="counts" role="group" aria-label="Show">
            {(["movie", "tv"] as const).map((k) => (
              <button key={k} className={kind === k ? "on" : ""} onClick={() => setKind(k)}>
                <span>{k === "movie" ? "Movies" : "Series"}</span>
              </button>
            ))}
          </div>
          <PagedGrid path={`/browse/company/${id}?kind=${kind}`} />
        </>
      )}
    </>
  );
}
