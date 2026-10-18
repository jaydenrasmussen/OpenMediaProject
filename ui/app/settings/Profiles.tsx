"use client";

import { LoadState } from "../components/Page";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { QualityProfile } from "../lib/types";

// Quality ids are Radarr's. Only what the summary needs.
const NAMES: Record<number, string> = {
  0: "Unknown", 1: "SDTV", 2: "DVD", 3: "WEBDL-1080p", 4: "HDTV-720p", 5: "WEBDL-720p", 6: "Bluray-720p",
  7: "Bluray-1080p", 8: "WEBDL-480p", 9: "HDTV-1080p", 10: "Raw-HD", 12: "WEBRip-480p", 14: "WEBRip-720p",
  15: "WEBRip-1080p", 16: "HDTV-2160p", 17: "WEBRip-2160p", 18: "WEBDL-2160p", 19: "Bluray-2160p",
  20: "Bluray-480p", 21: "Bluray-576p", 22: "BR-DISK", 23: "DVD-R", 24: "WORKPRINT", 25: "CAM",
  26: "TELESYNC", 27: "TELECINE", 28: "DVDSCR", 29: "REGIONAL", 30: "Remux-1080p", 31: "Remux-2160p",
};

function summary(p: QualityProfile): { allowed: string[]; cutoff: string } {
  const allowed: string[] = [];
  let cutoff = NAMES[p.cutoff] ?? `#${p.cutoff}`;
  for (const item of p.items) {
    if (item.Quality?.allowed) allowed.push(NAMES[item.Quality.quality] ?? `#${item.Quality.quality}`);
    if (item.Group) {
      if (item.Group.allowed) allowed.push(item.Group.name);
      if (item.Group.id === p.cutoff) cutoff = item.Group.name;
    }
  }
  return { allowed, cutoff };
}

export function Profiles() {
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  return (
    <div className="stack">
      <p className="muted small">
        These came with the install. Creating and editing profiles is available through the API for now (<code>/api/v1/qualityprofile</code>).
      </p>
      <LoadState loading={profiles.loading} error={profiles.error} hasData={!!profiles.data} />
      {profiles.data?.map((p) => {
        const s = summary(p);
        return (
          <div key={p.id} className="card">
            <div className="spread">
              <strong>{p.name}</strong>
              <span className="muted small">
                Upgrades until <strong>{s.cutoff}</strong>
                {p.upgrade_allowed ? "" : " (upgrades off)"}
              </span>
            </div>
            <div className="row" style={{ marginTop: ".5rem" }}>
              {s.allowed.map((a) => (
                <span key={a} className="badge">
                  {a}
                </span>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
