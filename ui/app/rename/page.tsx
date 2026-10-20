"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";

interface Preview {
  movie_id: number;
  title: string;
  existing: string;
  new: string;
}

export default function RenamePage() {
  return (
    <Suspense fallback={<p className="muted">Loading…</p>}>
      <Rename />
    </Suspense>
  );
}

function Rename() {
  const notify = useToast();
  const movieId = useSearchParams().get("id");
  const previews = useLoad(() => get<Preview[]>(movieId ? `/rename?movie_id=${movieId}` : "/rename"), [movieId]);
  const [busy, setBusy] = useState(false);

  async function apply() {
    setBusy(true);
    try {
      await post("/command", movieId ? { name: "RenameMovie", movie_id: Number(movieId), wait: true } : { name: "RenameMovies", wait: true });
      notify("Renamed.", "good");
      previews.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1>Rename files</h1>
      <p className="muted small">
        Files are renamed to match the naming format in <Link href="/settings/">Settings</Link>. Renaming has to be switched on there first.
      </p>
      <LoadState loading={previews.loading} error={previews.error} hasData={!!previews.data} />
      {previews.data && previews.data.length === 0 && <p className="muted">Every file already has the right name.</p>}
      {previews.data && previews.data.length > 0 && (
        <>
          <div className="stack">
            {previews.data.map((p) => (
              <div key={p.movie_id} className="card">
                <Link href={`/movie/?id=${p.movie_id}`}>
                  <strong>{p.title}</strong>
                </Link>
                <div className="small muted" style={{ wordBreak: "break-all" }}>
                  {p.existing}
                </div>
                <div className="small" style={{ wordBreak: "break-all" }}>
                  → {p.new}
                </div>
              </div>
            ))}
          </div>
          <div className="row end" style={{ marginTop: "1rem" }}>
            <button className="primary" onClick={apply} disabled={busy}>
              {busy ? "Renaming…" : `Rename ${previews.data.length} file${previews.data.length === 1 ? "" : "s"}`}
            </button>
          </div>
        </>
      )}
    </>
  );
}
