"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { useToast } from "../components/toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { CollectionDetail, CollectionSummary, QualityProfile, RootFolder } from "../lib/types";

export default function Collections() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <Router />
    </Suspense>
  );
}

function Router() {
  const id = useSearchParams().get("id");
  return id ? <Detail id={Number(id)} /> : <List />;
}

function List() {
  const list = useLoad(() => get<CollectionSummary[]>("/collection"));
  return (
    <>
      <h1>Collections</h1>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data?.length === 0 && <p className="muted">None of your movies belong to a collection yet.</p>}
      <div className="stack">
        {list.data?.map((c) => (
          <div key={c.tmdb_id} className="card spread">
            <Link href={`/collections/?id=${c.tmdb_id}`}>{c.title}</Link>
            <span className="muted">{c.movies_in_library} in library</span>
          </div>
        ))}
      </div>
    </>
  );
}

function Detail({ id }: { id: number }) {
  const notify = useToast();
  const detail = useLoad(() => get<CollectionDetail>(`/collection/${id}`), [id]);
  const roots = useLoad(() => get<RootFolder[]>("/rootfolder"));
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const [busy, setBusy] = useState(false);
  const root = roots.data?.[0]?.path;
  const profile = profiles.data?.[0]?.id;
  const missing = detail.data?.movies.filter((m) => !m.in_library).length ?? 0;

  async function addMissing() {
    setBusy(true);
    try {
      const r = await post<{ added: number[] }>(`/collection/${id}/add`, {
        root_folder_path: root,
        quality_profile_id: profile,
        search_now: true,
      });
      notify(`Added ${r.added.length} movie${r.added.length === 1 ? "" : "s"}`, "good");
      detail.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <p className="small">
        <Link href="/collections/">← Collections</Link>
      </p>
      <LoadState loading={detail.loading} error={detail.error} hasData={!!detail.data} />
      {detail.data && (
        <>
          <div className="spread">
            <h1>{detail.data.title}</h1>
            <button className="primary" onClick={addMissing} disabled={busy || missing === 0 || !root || profile === undefined}>
              {busy ? "Adding…" : `Add ${missing} missing`}
            </button>
          </div>
          {detail.data.overview && <p className="muted">{detail.data.overview}</p>}
          <div className="stack">
            {detail.data.movies.map((m) => (
              <div key={m.tmdb_id} className="card row" style={{ flexWrap: "nowrap", alignItems: "flex-start" }}>
                <div style={{ width: 70, flexShrink: 0 }}>
                  <Poster images={m.images} title={m.title} width={154} />
                </div>
                <div className="stack" style={{ flex: 1 }}>
                  <strong>
                    {m.title} {m.year ? <span className="muted">({m.year})</span> : null}
                  </strong>
                  <p className="muted small">{m.overview}</p>
                </div>
                {m.in_library ? <Link href={`/movie/?id=${m.in_library}`}>In library</Link> : <span className="muted">Missing</span>}
              </div>
            ))}
          </div>
        </>
      )}
    </>
  );
}
