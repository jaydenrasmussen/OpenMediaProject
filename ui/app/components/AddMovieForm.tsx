"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { LoadState } from "./Page";
import { useToast } from "./toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Movie, QualityProfile, RootFolder } from "../lib/types";

export function AddMovieForm({ tmdbId }: { tmdbId: number }) {
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
