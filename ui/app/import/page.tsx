"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { LookupResult, QualityProfile, RootFolder } from "../lib/types";

interface Unmapped {
  name: string;
  path: string;
  title: string;
  year: number | null;
  has_media: boolean;
}

interface Row extends Unmapped {
  match: LookupResult | null;
  state: "waiting" | "looking" | "done";
  include: boolean;
  result?: { ok: boolean; error?: string; movie_id?: number };
}

interface ImportResponse {
  results: { path: string; ok: boolean; error?: string; movie_id?: number }[];
}

export default function ImportLibrary() {
  const notify = useToast();
  const roots = useLoad(() => get<RootFolder[]>("/rootfolder"));
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const [rootId, setRootId] = useState<number | null>(null);
  const [rows, setRows] = useState<Row[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [profile, setProfile] = useState<number | "">("");
  const [monitored, setMonitored] = useState(true);
  const [importing, setImporting] = useState(false);
  const generation = useRef(0);

  const selectedRoot = rootId ?? roots.data?.[0]?.id ?? null;
  const profileValue = profile === "" ? (profiles.data?.[0]?.id ?? "") : profile;

  async function scan() {
    if (selectedRoot == null) return;
    const mine = ++generation.current;
    setScanning(true);
    setRows(null);
    try {
      const found = await get<Unmapped[]>(`/rootfolder/${selectedRoot}/unmapped`);
      if (mine !== generation.current) return;
      const initial: Row[] = found.map((f) => ({ ...f, match: null, state: "waiting", include: false }));
      setRows(initial);
      lookUp(initial, mine);
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setScanning(false);
    }
  }

  /** Looks each folder up on TMDB, a few at a time. */
  async function lookUp(initial: Row[], mine: number) {
    const queue = initial.map((_, i) => i);
    const worker = async () => {
      for (let i = queue.shift(); i !== undefined; i = queue.shift()) {
        if (mine !== generation.current) return;
        const row = initial[i];
        update(i, { state: "looking" });
        try {
          const term = row.year ? `${row.title} ${row.year}` : row.title;
          const found = await get<LookupResult[]>(`/movie/lookup?term=${encodeURIComponent(term)}`);
          const best = found[0] ?? null;
          update(i, { match: best, state: "done", include: !!best && !best.in_library && row.has_media });
        } catch {
          update(i, { state: "done" });
        }
      }
    };
    await Promise.all([worker(), worker(), worker(), worker()]);
  }

  function update(index: number, patch: Partial<Row>) {
    setRows((r) => r && r.map((row, i) => (i === index ? { ...row, ...patch } : row)));
  }

  async function pick(index: number, term: string) {
    const t = term.trim();
    if (!t) return;
    update(index, { state: "looking" });
    try {
      const found = await get<LookupResult[]>(`/movie/lookup?term=${encodeURIComponent(t)}`);
      update(index, { match: found[0] ?? null, state: "done", include: !!found[0] && !found[0].in_library });
    } catch (e) {
      notify(errorMessage(e), "bad");
      update(index, { state: "done" });
    }
  }

  async function run() {
    if (!rows) return;
    const chosen = rows.filter((r) => r.include && r.match && !r.result?.ok);
    if (chosen.length === 0) return;
    setImporting(true);
    try {
      const res = await post<ImportResponse>("/movie/import", {
        items: chosen.map((r) => ({ path: r.path, tmdb_id: r.match!.tmdb_id })),
        quality_profile_id: profileValue,
        monitored,
      });
      const byPath = new Map(res.results.map((r) => [r.path, r]));
      setRows((all) => all && all.map((r) => (byPath.has(r.path) ? { ...r, result: byPath.get(r.path), include: !byPath.get(r.path)!.ok } : r)));
      const ok = res.results.filter((r) => r.ok).length;
      notify(`Imported ${ok} of ${res.results.length}.`, ok === res.results.length ? "good" : "info");
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setImporting(false);
    }
  }

  // Start scanning as soon as there is a root folder to scan.
  useEffect(() => {
    if (selectedRoot != null && rows === null && !scanning) void scan();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedRoot]);

  const count = rows?.filter((r) => r.include && r.match && !r.result?.ok).length ?? 0;
  return (
    <>
      <h1>Import existing movies</h1>
      <p className="muted">
        Finds movie folders in a root folder that are not in the library, matches them on TMDB, and adds them where they are. Files inside are picked up automatically.
      </p>
      <LoadState loading={roots.loading} error={roots.error} hasData={!!roots.data} />
      {roots.data?.length === 0 && (
        <p className="warn">
          There is no root folder yet. <Link href="/settings/">Add one in Settings</Link>.
        </p>
      )}
      {roots.data && roots.data.length > 0 && (
        <div className="row">
          <label className="field">
            <span>Root folder</span>
            <select
              value={selectedRoot ?? ""}
              onChange={(e) => {
                setRootId(Number(e.target.value));
                setRows(null);
              }}
            >
              {roots.data.map((r) => (
                <option key={r.id} value={r.id}>
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
          <label className="check" style={{ alignSelf: "end", paddingBottom: ".5rem" }}>
            <input type="checkbox" checked={monitored} onChange={(e) => setMonitored(e.target.checked)} /> Monitored
          </label>
          <button style={{ alignSelf: "end" }} onClick={scan} disabled={scanning}>
            {scanning ? "Scanning…" : "Scan again"}
          </button>
        </div>
      )}

      {rows && rows.length === 0 && <p className="muted">Every folder in this root folder is already in the library.</p>}
      {rows && rows.length > 0 && (
        <>
          <div className="table-wrap" style={{ marginTop: "1rem" }}>
            <table>
              <thead>
                <tr>
                  <th />
                  <th>Folder</th>
                  <th>Matched to</th>
                  <th>Change match</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={r.path}>
                    <td>
                      <input
                        type="checkbox"
                        aria-label={`Import ${r.name}`}
                        checked={r.include}
                        disabled={!r.match || !!r.result?.ok}
                        onChange={(e) => update(i, { include: e.target.checked })}
                      />
                    </td>
                    <td style={{ wordBreak: "break-word" }}>
                      {r.name}
                      {!r.has_media && <div className="small warn">No video files inside</div>}
                    </td>
                    <td>
                      {r.result?.ok ? (
                        <Link href={`/movie/?id=${r.result.movie_id}`}>Added</Link>
                      ) : r.state !== "done" ? (
                        <span className="muted">Looking…</span>
                      ) : r.match ? (
                        <>
                          {r.match.title} <span className="muted">({r.match.year || "–"})</span>
                          {r.match.in_library && <div className="small warn">Already in the library</div>}
                        </>
                      ) : (
                        <span className="warn">No match</span>
                      )}
                      {r.result && !r.result.ok && <div className="small error">{r.result.error}</div>}
                    </td>
                    <td>
                      <form
                        className="row"
                        onSubmit={(e) => {
                          e.preventDefault();
                          const input = e.currentTarget.elements.namedItem("term") as HTMLInputElement;
                          void pick(i, input.value);
                        }}
                      >
                        <input name="term" type="text" placeholder="title or tmdb:603" aria-label={`Search for ${r.name}`} style={{ width: 160 }} />
                        <button>Find</button>
                      </form>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="row end" style={{ marginTop: "1rem" }}>
            <span className="muted small">{count} selected</span>
            <button className="primary" onClick={run} disabled={importing || count === 0 || profileValue === ""}>
              {importing ? "Importing…" : `Import ${count} movie${count === 1 ? "" : "s"}`}
            </button>
          </div>
        </>
      )}
    </>
  );
}
