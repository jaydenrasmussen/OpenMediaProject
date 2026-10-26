"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { CustomFilterBar } from "../components/CustomFilterBar";
import { LoadState } from "../components/Page";
import { Poster } from "../components/Poster";
import { useToast } from "../components/toast";
import { get, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import { useLive } from "../lib/live";
import type { QualityProfile, Series } from "../lib/types";

type Filter = "all" | "continuing" | "ended" | "missing" | "unmonitored";
type Sort = "title" | "year" | "added" | "next";
type SavedView = { filter: Filter; sort: Sort };

function badge(s: Series) {
  if (!s.monitored) return <span className="badge">Unmonitored</span>;
  if (s.missing_episode_count > 0) return <span className="badge warn">{s.missing_episode_count} missing</span>;
  if (s.episode_count > 0 && s.episode_file_count === s.episode_count) return <span className="badge good">Complete</span>;
  return <span className="badge accent">{s.status === "ended" ? "Ended" : "Continuing"}</span>;
}

export default function SeriesLibrary() {
  const { data, error, loading, reload } = useLoad(() => get<Series[]>("/series"));
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  useLive(reload, ["history"]);
  const notify = useToast();
  const [editing, setEditing] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [change, setChange] = useState({ monitored: "", profile: "", type: "", season_folder: "" });
  const [saving, setSaving] = useState(false);
  const [text, setText] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("title");

  const shows = useMemo(() => {
    const q = text.trim().toLowerCase();
    return (data ?? [])
      .filter((s) => !q || s.title.toLowerCase().includes(q))
      .filter((s) => {
        switch (filter) {
          case "continuing":
            return s.status === "continuing";
          case "ended":
            return s.status === "ended";
          case "missing":
            return s.monitored && s.missing_episode_count > 0;
          case "unmonitored":
            return !s.monitored;
          default:
            return true;
        }
      })
      .sort((a, b) => {
        if (sort === "year") return b.year - a.year || a.title.localeCompare(b.title);
        if (sort === "added") return b.added.localeCompare(a.added);
        if (sort === "next") return (a.next_airing ?? "9").localeCompare(b.next_airing ?? "9") || a.title.localeCompare(b.title);
        return a.title.localeCompare(b.title);
      });
  }, [data, text, filter, sort]);

  async function apply() {
    const body: Record<string, unknown> = { series_ids: [...picked] };
    if (change.monitored) body.monitored = change.monitored === "yes";
    if (change.profile) body.quality_profile_id = Number(change.profile);
    if (change.type) body.series_type = change.type;
    if (change.season_folder) body.season_folder = change.season_folder === "yes";
    setSaving(true);
    try {
      await put("/series/editor", body);
      notify(`Updated ${picked.size} series.`, "good");
      setPicked(new Set());
      reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setSaving(false);
    }
  }
  const toggle = (id: number) =>
    setPicked((p) => {
      const next = new Set(p);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const anyChange = Object.values(change).some(Boolean);

  return (
    <>
      <div className="spread">
        <h1>Series</h1>
        <div className="row">
          <button
            onClick={() => {
              setEditing((v) => !v);
              setPicked(new Set());
            }}
          >
            {editing ? "Done" : "Edit"}
          </button>
          <input type="search" placeholder="Filter by title" value={text} onChange={(e) => setText(e.target.value)} aria-label="Filter by title" />
          <select value={filter} onChange={(e) => setFilter(e.target.value as Filter)} aria-label="Show">
            <option value="all">All</option>
            <option value="continuing">Continuing</option>
            <option value="ended">Ended</option>
            <option value="missing">Missing episodes</option>
            <option value="unmonitored">Unmonitored</option>
          </select>
          <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort by">
            <option value="title">Title</option>
            <option value="year">Year</option>
            <option value="added">Recently added</option>
            <option value="next">Next airing</option>
          </select>
          <CustomFilterBar<SavedView>
            filterType="series"
            state={{ filter, sort }}
            onApply={(v) => {
              setFilter(v.filter);
              setSort(v.sort);
            }}
          />
        </div>
      </div>

      {editing && (
        <section className="card stack" style={{ marginBottom: "1rem" }}>
          <div className="spread">
            <strong>{picked.size} selected</strong>
            <div className="row">
              <button onClick={() => setPicked(new Set(shows.map((s) => s.id)))}>Select all shown</button>
              <button onClick={() => setPicked(new Set())}>Clear</button>
            </div>
          </div>
          <div className="form-grid">
            <label className="field">
              <span>Monitored</span>
              <select value={change.monitored} onChange={(e) => setChange({ ...change, monitored: e.target.value })}>
                <option value="">Leave as it is</option>
                <option value="yes">Monitored</option>
                <option value="no">Unmonitored</option>
              </select>
            </label>
            <label className="field">
              <span>Quality profile</span>
              <select value={change.profile} onChange={(e) => setChange({ ...change, profile: e.target.value })}>
                <option value="">Leave as it is</option>
                {profiles.data?.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>Series type</span>
              <select value={change.type} onChange={(e) => setChange({ ...change, type: e.target.value })}>
                <option value="">Leave as it is</option>
                <option value="standard">Standard</option>
                <option value="daily">Daily</option>
                <option value="anime">Anime</option>
              </select>
            </label>
            <label className="field">
              <span>Season folders</span>
              <select value={change.season_folder} onChange={(e) => setChange({ ...change, season_folder: e.target.value })}>
                <option value="">Leave as it is</option>
                <option value="yes">Yes</option>
                <option value="no">No</option>
              </select>
            </label>
          </div>
          <div className="row end">
            <button className="primary" onClick={apply} disabled={saving || picked.size === 0 || !anyChange}>
              {saving ? "Saving…" : `Apply to ${picked.size}`}
            </button>
          </div>
        </section>
      )}

      <LoadState loading={loading} error={error} hasData={!!data} />
      {data && data.length === 0 && (
        <div className="card">
          <p>No series yet.</p>
          <p>
            <Link href="/settings/">Add a root folder</Link>, then <Link href="/series/add/">add your first series</Link>.
          </p>
        </div>
      )}
      {data && data.length > 0 && shows.length === 0 && <p className="muted">No series match.</p>}

      <div className="grid">
        {shows.map((s) => (
          <Link
            key={s.id}
            href={`/series/show/?id=${s.id}`}
            className="poster-card"
            style={editing && picked.has(s.id) ? { outline: "3px solid var(--accent, #4c8bf5)" } : undefined}
            onClick={(e) => {
              if (editing) {
                e.preventDefault();
                toggle(s.id);
              }
            }}
          >
            <Poster images={s.images} title={s.title} />
            <div className="poster-meta">
              <div className="title" title={s.title}>
                {s.title}
              </div>
              <div className="row small muted" style={{ justifyContent: "space-between" }}>
                <span>
                  {s.episode_file_count}/{s.episode_count}
                </span>
                {badge(s)}
              </div>
            </div>
          </Link>
        ))}
      </div>
    </>
  );
}
