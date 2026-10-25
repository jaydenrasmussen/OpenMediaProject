"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { LoadState } from "./Page";
import { useToast } from "./toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { QualityProfile, RootFolder, Series } from "../lib/types";

const MONITOR_CHOICES = [
  ["all", "All episodes"],
  ["future", "Future episodes"],
  ["missing", "Missing episodes"],
  ["existing", "Existing episodes"],
  ["recent", "Recent episodes"],
  ["pilot", "Pilot episode only"],
  ["first_season", "First season"],
  ["last_season", "Last season"],
  ["none", "None"],
];

export function AddSeriesForm({ tmdbId }: { tmdbId: number }) {
  const router = useRouter();
  const notify = useToast();
  const roots = useLoad(() => get<RootFolder[]>("/rootfolder"));
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const [root, setRoot] = useState("");
  const [profile, setProfile] = useState<number | "">("");
  const [monitor, setMonitor] = useState("all");
  const [type, setType] = useState("");
  const [seasonFolder, setSeasonFolder] = useState(true);
  const [searchNow, setSearchNow] = useState(false);
  const [busy, setBusy] = useState(false);

  const rootValue = root || roots.data?.[0]?.path || "";
  const profileValue = profile === "" ? (profiles.data?.[0]?.id ?? "") : profile;

  async function add() {
    setBusy(true);
    try {
      const series = await post<Series>("/series", {
        tmdb_id: tmdbId,
        root_folder_path: rootValue,
        quality_profile_id: profileValue,
        monitor,
        series_type: type || undefined,
        season_folder: seasonFolder,
        search_now: searchNow,
      });
      router.push(`/series/show/?id=${series.id}`);
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
          <span>Monitor</span>
          <select value={monitor} onChange={(e) => setMonitor(e.target.value)}>
            {MONITOR_CHOICES.map(([v, label]) => (
              <option key={v} value={v}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Series type</span>
          <select value={type} onChange={(e) => setType(e.target.value)}>
            <option value="">Let the provider suggest</option>
            <option value="standard">Standard (season and episode)</option>
            <option value="daily">Daily (by air date)</option>
            <option value="anime">Anime (absolute numbers)</option>
          </select>
        </label>
      </div>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={seasonFolder} onChange={(e) => setSeasonFolder(e.target.checked)} /> Season folders
        </label>
      <label className="check">
        <input type="checkbox" checked={searchNow} onChange={(e) => setSearchNow(e.target.checked)} /> Search for missing episodes now
      </label>
        <button className="primary" onClick={add} disabled={busy || !rootValue || profileValue === ""}>
          {busy ? "Adding…" : "Add series"}
        </button>
      </div>
    </div>
  );
}
