"use client";

import Link from "next/link";
import { useMemo, useState } from "react";
import { LoadState } from "./components/Page";
import { useToast } from "./components/toast";
import { Counts } from "./components/Counts";
import { Hero } from "./components/Hero";
import { Poster } from "./components/Poster";
import { api, get, put } from "./lib/api";
import { errorMessage, useLoad } from "./lib/hooks";
import { useLive } from "./lib/live";
import type { Movie, QualityProfile } from "./lib/types";

type Filter = "all" | "wanted" | "downloaded" | "unmonitored";
type Sort = "title" | "year" | "added";

function badge(m: Movie) {
  if (m.has_file) return <span className="badge good">Downloaded</span>;
  if (!m.monitored) return <span className="badge">Unmonitored</span>;
  return <span className="badge warn">Wanted</span>;
}

export default function Library() {
  const { data, error, loading, reload } = useLoad(() => get<Movie[]>("/movie"));
  useLive(reload, ["history"]);
  const [text, setText] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("title");
  const notify = useToast();
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const [editing, setEditing] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [change, setChange] = useState({ monitored: "", profile: "", availability: "" });
  const [saving, setSaving] = useState(false);
  const anyChange = Object.values(change).some(Boolean);

  const toggle = (id: number) =>
    setPicked((p) => {
      const next = new Set(p);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  async function apply() {
    const body: Record<string, unknown> = { movie_ids: [...picked] };
    if (change.monitored) body.monitored = change.monitored === "yes";
    if (change.profile) body.quality_profile_id = Number(change.profile);
    if (change.availability) body.minimum_availability = change.availability;
    setSaving(true);
    try {
      await put("/movie/editor", body);
      notify(`Updated ${picked.size} movies.`, "good");
      setPicked(new Set());
      reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setSaving(false);
    }
  }

  async function removePicked() {
    if (picked.size === 0 || !confirm(`Remove ${picked.size} movies from the library?`)) return;
    const alsoFiles = confirm("Also delete their folders from disk? OK deletes them; Cancel keeps the files.");
    setSaving(true);
    try {
      await api("/movie/editor", { method: "DELETE", body: { movie_ids: [...picked], delete_files: alsoFiles } });
      notify(`Removed ${picked.size} movies.`, "good");
      setPicked(new Set());
      reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setSaving(false);
    }
  }

  const movies = useMemo(() => {
    const q = text.trim().toLowerCase();
    return (data ?? [])
      .filter((m) => !q || m.title.toLowerCase().includes(q))
      .filter((m) => {
        switch (filter) {
          case "wanted":
            return m.monitored && !m.has_file;
          case "downloaded":
            return m.has_file;
          case "unmonitored":
            return !m.monitored;
          default:
            return true;
        }
      })
      .sort((a, b) => {
        if (sort === "year") return b.year - a.year || a.title.localeCompare(b.title);
        if (sort === "added") return b.added.localeCompare(a.added);
        return a.title.localeCompare(b.title);
      });
  }, [data, text, filter, sort]);

  // The newest addition that has a backdrop to show.
  const featured = useMemo(
    () => [...(data ?? [])].sort((a, b) => b.added.localeCompare(a.added)).find((m) => m.images.some((i) => i.kind === "Fanart")),
    [data],
  );

  return (
    <>
      {featured && !text && (
        <Hero
          images={featured.images}
          eyebrow="Recently added"
          title={featured.title}
          meta={[featured.year || null, featured.runtime ? `${featured.runtime} min` : null, featured.genres.slice(0, 3).join(" · ") || null].filter(Boolean).join("  ·  ")}
          overview={featured.overview}
        >
          <Link href={`/movie/?id=${featured.id}`} className="button primary">
            Open
          </Link>
        </Hero>
      )}
      <div className="spread">
        <h1 className="shelf-title">Movies</h1>
        <div className="row">
          <button
            onClick={() => {
              setEditing((v) => !v);
              setPicked(new Set());
            }}
          >
            {editing ? "Done" : "Edit"}
          </button>
          <input
            type="search"
            placeholder="Filter by title"
            value={text}
            onChange={(e) => setText(e.target.value)}
            aria-label="Filter by title"
          />
          <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort by">
            <option value="title">Title</option>
            <option value="year">Year</option>
            <option value="added">Recently added</option>
          </select>
        </div>
      </div>

      <Counts
        value={filter}
        onChange={setFilter}
        options={[
          { id: "all", label: "All", count: data?.length ?? 0 },
          { id: "downloaded", label: "Downloaded", count: data?.filter((m) => m.has_file).length ?? 0 },
          { id: "wanted", label: "Wanted", count: data?.filter((m) => m.monitored && !m.has_file).length ?? 0 },
          { id: "unmonitored", label: "Unmonitored", count: data?.filter((m) => !m.monitored).length ?? 0 },
        ]}
      />

      {editing && (
        <section className="card stack" style={{ marginBottom: "1rem" }}>
          <div className="spread">
            <strong>{picked.size} selected</strong>
            <div className="row">
              <button onClick={() => setPicked(new Set(movies.map((m) => m.id)))}>Select all shown</button>
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
              <span>Minimum availability</span>
              <select value={change.availability} onChange={(e) => setChange({ ...change, availability: e.target.value })}>
                <option value="">Leave as it is</option>
                <option value="announced">Announced</option>
                <option value="inCinemas">In cinemas</option>
                <option value="released">Released</option>
              </select>
            </label>
          </div>
          <div className="row end">
            <button className="primary" onClick={apply} disabled={saving || picked.size === 0 || !anyChange}>
              {saving ? "Saving…" : `Apply to ${picked.size}`}
            </button>
            <button className="danger" onClick={removePicked} disabled={saving || picked.size === 0}>
              Remove {picked.size}
            </button>
          </div>
        </section>
      )}

      <LoadState loading={loading} error={error} hasData={!!data} />

      {data && data.length === 0 && (
        <div className="card">
          <p>Your library is empty.</p>
          <p>
            <Link href="/settings/">Add a root folder</Link>, then{" "}
            <Link href="/add/">add your first movie</Link>.
          </p>
        </div>
      )}
      {data && data.length > 0 && movies.length === 0 && <p className="muted">No movies match.</p>}

      <div className="grid">
        {movies.map((m) => (
          <Link
            key={m.id}
            href={`/movie/?id=${m.id}`}
            className="poster-card"
            style={editing && picked.has(m.id) ? { outline: "3px solid var(--accent, #4c8bf5)" } : undefined}
            onClick={(e) => {
              if (editing) {
                e.preventDefault();
                toggle(m.id);
              }
            }}
          >
            <Poster images={m.images} title={m.title} />
            <div className="poster-meta">
              <div className="title" title={m.title}>
                {m.title}
              </div>
              <div className="row small muted" style={{ justifyContent: "space-between" }}>
                <span>{m.year || "–"}</span>
                {badge(m)}
              </div>
              {m.media_servers.length > 0 && <div className="small good">On {m.media_servers.join(", ")}</div>}
            </div>
          </Link>
        ))}
      </div>
    </>
  );
}
