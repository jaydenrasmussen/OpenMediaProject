"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { useToast } from "../components/toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { LookupResult, Movie, QualityProfile, RootFolder } from "../lib/types";

export default function AddMovie() {
  const [term, setTerm] = useState("");
  const [results, setResults] = useState<LookupResult[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<number | null>(null);

  async function search(e: React.FormEvent) {
    e.preventDefault();
    if (!term.trim()) return;
    setSearching(true);
    setError(null);
    setOpened(null);
    try {
      setResults(await get<LookupResult[]>(`/movie/lookup?term=${encodeURIComponent(term.trim())}`));
    } catch (err) {
      setResults(null);
      setError(errorMessage(err));
    } finally {
      setSearching(false);
    }
  }

  return (
    <>
      <h1>Add movie</h1>
      <form className="row" onSubmit={search}>
        <input
          type="search"
          style={{ flex: 1, minWidth: 220 }}
          placeholder="Title, “title year”, tmdb:603 or imdb:tt0133093"
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          autoFocus
          aria-label="Search for a movie"
        />
        <button className="primary" disabled={searching}>
          {searching ? "Searching…" : "Search"}
        </button>
      </form>
      {error && <p className="error">{error}</p>}
      {results && results.length === 0 && <p className="muted">Nothing found.</p>}

      <div className="stack" style={{ marginTop: "1rem" }}>
        {results?.map((r) => (
          <div key={r.tmdb_id} className="card">
            <div className="row" style={{ alignItems: "flex-start", flexWrap: "nowrap" }}>
              <div style={{ width: 90, flexShrink: 0 }}>
                <Poster images={r.images} title={r.title} width={154} />
              </div>
              <div className="stack" style={{ flex: 1, minWidth: 0 }}>
                <div className="spread">
                  <strong>
                    {r.title} {r.year ? <span className="muted">({r.year})</span> : null}
                  </strong>
                  {r.in_library ? (
                    <Link href={`/movie/?id=${r.in_library}`}>In library</Link>
                  ) : (
                    <button onClick={() => setOpened(opened === r.tmdb_id ? null : r.tmdb_id)}>
                      {opened === r.tmdb_id ? "Cancel" : "Add"}
                    </button>
                  )}
                </div>
                <p className="muted small" style={{ maxHeight: "4.5em", overflow: "hidden" }}>
                  {r.overview}
                </p>
                {opened === r.tmdb_id && <AddForm tmdbId={r.tmdb_id} />}
              </div>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function AddForm({ tmdbId }: { tmdbId: number }) {
  const router = useRouter();
  const notify = useToast();
  const roots = useLoad(() => get<RootFolder[]>("/rootfolder"));
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const [root, setRoot] = useState("");
  const [profile, setProfile] = useState<number | "">("");
  const [monitored, setMonitored] = useState(true);
  const [searchNow, setSearchNow] = useState(true);
  const [availability, setAvailability] = useState("announced");
  const [busy, setBusy] = useState(false);

  const rootValue = root || roots.data?.[0]?.path || "";
  const profileValue = profile === "" ? (profiles.data?.[0]?.id ?? "") : profile;

  async function add() {
    setBusy(true);
    try {
      const movie = await post<Movie>("/movie", {
        tmdb_id: tmdbId,
        root_folder_path: rootValue,
        quality_profile_id: profileValue,
        monitored,
        minimum_availability: availability,
        search_now: searchNow,
      });
      router.push(`/movie/?id=${movie.id}`);
    } catch (err) {
      notify(errorMessage(err), "bad");
      setBusy(false);
    }
  }

  if (roots.data && roots.data.length === 0) {
    return (
      <p className="warn">
        There is no root folder yet. <Link href="/settings/">Add one in Settings</Link> first.
      </p>
    );
  }
  return (
    <div className="stack">
      <LoadState loading={roots.loading || profiles.loading} error={roots.error ?? profiles.error} hasData={!!roots.data && !!profiles.data} />
      <div className="form-grid">
        <label className="field">
          <span>Root folder</span>
          <select value={rootValue} onChange={(e) => setRoot(e.target.value)}>
            {roots.data?.map((r) => (
              <option key={r.id} value={r.path}>
                {r.path}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Quality profile</span>
          <select value={profileValue} onChange={(e) => setProfile(Number(e.target.value))}>
            {profiles.data?.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Minimum availability</span>
          <select value={availability} onChange={(e) => setAvailability(e.target.value)}>
            <option value="announced">Announced</option>
            <option value="inCinemas">In cinemas</option>
            <option value="released">Released</option>
          </select>
        </label>
      </div>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={monitored} onChange={(e) => setMonitored(e.target.checked)} /> Monitored
        </label>
        <label className="check">
          <input type="checkbox" checked={searchNow} onChange={(e) => setSearchNow(e.target.checked)} disabled={!monitored} />{" "}
          Search for it now
        </label>
        <button className="primary" onClick={add} disabled={busy || !rootValue || profileValue === ""}>
          {busy ? "Adding…" : "Add movie"}
        </button>
      </div>
    </div>
  );
}
