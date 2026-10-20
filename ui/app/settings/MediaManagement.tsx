"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import { Decision } from "./Decision";
import { formatSize } from "../lib/format";
import type { ImportConfig, RootFolder } from "../lib/types";

export function MediaManagement() {
  return (
    <div className="stack">
      <RootFolders />
      <Decision />
      <Naming />
    </div>
  );
}

function RootFolders() {
  const notify = useToast();
  const folders = useLoad(() => get<RootFolder[]>("/rootfolder"));
  const [path, setPath] = useState("");

  async function add(e: React.FormEvent) {
    e.preventDefault();
    try {
      await post("/rootfolder", { path: path.trim() });
      setPath("");
      folders.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function remove(f: RootFolder) {
    if (!confirm(`Stop using ${f.path}? Movies already there stay in the library.`)) return;
    try {
      await del(`/rootfolder/${f.id}`);
      folders.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Root folders</h2>
      <p className="muted small">New movies get their own folder inside one of these. The folder must exist and be writable by the server.</p>
      <LoadState loading={folders.loading} error={folders.error} hasData={!!folders.data} />
      {folders.data?.map((f) => (
        <div key={f.id} className="spread">
          <span>
            <code>{f.path}</code>
            {f.free_space != null && <span className="muted small"> · {formatSize(f.free_space)} free</span>}
          </span>
          <button className="danger" onClick={() => remove(f)}>
            Remove
          </button>
        </div>
      ))}
      <form className="row" onSubmit={add}>
        <input type="text" style={{ flex: 1, minWidth: 220 }} placeholder="/media/movies" value={path} onChange={(e) => setPath(e.target.value)} aria-label="Folder path" />
        <button className="primary" disabled={!path.trim()}>
          Add folder
        </button>
      </form>
    </section>
  );
}

function Naming() {
  const notify = useToast();
  const loaded = useLoad(() => get<ImportConfig>("/config/import"));
  const [cfg, setCfg] = useState<ImportConfig | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (loaded.data) setCfg(loaded.data);
  }, [loaded.data]);

  if (!cfg) return <LoadState loading={loaded.loading} error={loaded.error} hasData={false} />;
  const set = (patch: Partial<ImportConfig>) => setCfg({ ...cfg, ...patch });
  const setNaming = (patch: Partial<ImportConfig["naming"]>) => setCfg({ ...cfg, naming: { ...cfg.naming, ...patch } });

  async function save() {
    setBusy(true);
    try {
      setCfg(await put<ImportConfig>("/config/import", cfg));
      notify("Saved.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Importing and naming</h2>
      <div className="form-grid">
        <label className="field">
          <span>Transfer files by</span>
          <select value={cfg.mode} onChange={(e) => set({ mode: e.target.value as ImportConfig["mode"] })}>
            <option value="auto">Moving, or copying while the client still seeds</option>
            <option value="move">Always moving</option>
            <option value="copy">Always copying</option>
          </select>
        </label>
        <label className="field">
          <span>Minimum free space (MB)</span>
          <input type="number" min={0} value={cfg.minimum_free_space_mb} onChange={(e) => set({ minimum_free_space_mb: Number(e.target.value) })} />
        </label>
      </div>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={cfg.copy_using_hardlinks} onChange={(e) => set({ copy_using_hardlinks: e.target.checked })} /> Use hardlinks instead of copying when possible
        </label>
        <label className="check">
          <input type="checkbox" checked={cfg.download_propers} onChange={(e) => set({ download_propers: e.target.checked })} /> Prefer propers and repacks
        </label>
        <label className="check">
          <input type="checkbox" checked={cfg.skip_free_space_check} onChange={(e) => set({ skip_free_space_check: e.target.checked })} /> Skip the free space check
        </label>
      </div>

      <h3>Removing files</h3>
      <div className="form-grid">
        <label className="field">
          <span>Recycle bin folder (empty deletes for good)</span>
          <input type="text" placeholder="/recycle" value={cfg.recycle_bin} onChange={(e) => set({ recycle_bin: e.target.value })} />
        </label>
        <label className="field">
          <span>Empty the bin after (days, 0 keeps everything)</span>
          <input type="number" min={0} value={cfg.recycle_bin_cleanup_days} onChange={(e) => set({ recycle_bin_cleanup_days: Number(e.target.value) })} />
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={cfg.redownload_failed} onChange={(e) => set({ redownload_failed: e.target.checked })} /> Look for another release when a download fails
      </label>
      <label className="check">
        <input type="checkbox" checked={cfg.unmonitor_deleted} onChange={(e) => set({ unmonitor_deleted: e.target.checked })} /> Stop monitoring a movie when its file is deleted
      </label>

      <h3>Naming</h3>
      <label className="check">
        <input type="checkbox" checked={cfg.naming.rename_movies} onChange={(e) => setNaming({ rename_movies: e.target.checked })} /> Rename imported movies
      </label>
      <div className="form-grid">
        <label className="field">
          <span>Movie file format</span>
          <input type="text" value={cfg.naming.standard_movie_format} onChange={(e) => setNaming({ standard_movie_format: e.target.value })} disabled={!cfg.naming.rename_movies} />
        </label>
        <label className="field">
          <span>Movie folder format</span>
          <input type="text" value={cfg.naming.movie_folder_format} onChange={(e) => setNaming({ movie_folder_format: e.target.value })} />
        </label>
        <label className="field">
          <span>Replace colons with</span>
          <select value={cfg.naming.colon_replacement} onChange={(e) => setNaming({ colon_replacement: e.target.value as ImportConfig["naming"]["colon_replacement"] })}>
            <option value="smart">Smart (“: ” becomes “ - ”)</option>
            <option value="delete">Delete</option>
            <option value="dash">Dash</option>
            <option value="space_dash">Space dash</option>
            <option value="space_dash_space">Space dash space</option>
          </select>
        </label>
      </div>
      <h3>Episode naming</h3>
      <label className="check">
        <input type="checkbox" checked={cfg.naming.rename_episodes} onChange={(e) => setNaming({ rename_episodes: e.target.checked })} /> Rename imported episodes
      </label>
      <div className="form-grid">
        <label className="field">
          <span>Standard episode format</span>
          <input type="text" value={cfg.naming.standard_episode_format} onChange={(e) => setNaming({ standard_episode_format: e.target.value })} disabled={!cfg.naming.rename_episodes} />
        </label>
        <label className="field">
          <span>Daily episode format</span>
          <input type="text" value={cfg.naming.daily_episode_format} onChange={(e) => setNaming({ daily_episode_format: e.target.value })} disabled={!cfg.naming.rename_episodes} />
        </label>
        <label className="field">
          <span>Anime episode format</span>
          <input type="text" value={cfg.naming.anime_episode_format} onChange={(e) => setNaming({ anime_episode_format: e.target.value })} disabled={!cfg.naming.rename_episodes} />
        </label>
        <label className="field">
          <span>Series folder format</span>
          <input type="text" value={cfg.naming.series_folder_format} onChange={(e) => setNaming({ series_folder_format: e.target.value })} />
        </label>
        <label className="field">
          <span>Season folder format</span>
          <input type="text" value={cfg.naming.season_folder_format} onChange={(e) => setNaming({ season_folder_format: e.target.value })} />
        </label>
        <label className="field">
          <span>Specials folder format</span>
          <input type="text" value={cfg.naming.specials_folder_format} onChange={(e) => setNaming({ specials_folder_format: e.target.value })} />
        </label>
        <label className="field">
          <span>Multi-episode style</span>
          <select value={cfg.naming.multi_episode_style} onChange={(e) => setNaming({ multi_episode_style: e.target.value as ImportConfig["naming"]["multi_episode_style"] })}>
            <option value="extend">Extend (S01E01-02-03)</option>
            <option value="duplicate">Duplicate (S01E01.S01E02)</option>
            <option value="repeat">Repeat (S01E01E02E03)</option>
            <option value="scene">Scene (S01E01-E02-E03)</option>
            <option value="range">Range (S01E01-03)</option>
            <option value="prefixed_range">Prefixed range (S01E01-E03)</option>
          </select>
        </label>
      </div>
      <p className="muted small">
        Tokens: <code>{"{Series Title}"}</code> <code>{"{Series TitleYear}"}</code> <code>{"{season:00}"}</code> <code>{"{episode:00}"}</code> <code>{"{absolute:000}"}</code>{" "}
        <code>{"{Episode Title}"}</code> <code>{"{Air-Date}"}</code> <code>{"{Quality Full}"}</code> <code>{"{Release Group}"}</code>… Sonarr’s formats work as they are.
      </p>
      <label className="check">
        <input type="checkbox" checked={cfg.naming.replace_illegal_characters} onChange={(e) => setNaming({ replace_illegal_characters: e.target.checked })} /> Replace illegal characters (otherwise they are removed)
      </label>
      <p className="small">
        <Link href="/rename/">Preview and rename files already in the library</Link>
      </p>
      <p className="muted small">
        Tokens: <code>{"{Movie Title}"}</code> <code>{"{Release Year}"}</code> <code>{"{Quality Full}"}</code> <code>{"{Release Group}"}</code>{" "}
        <code>{"{Edition Tags}"}</code> <code>{"{ImdbId}"}</code> <code>{"{TmdbId}"}</code> <code>{"{MediaInfo VideoCodec}"}</code>… Radarr’s formats work as they are.
      </p>
      <div className="row end">
        <button className="primary" onClick={save} disabled={busy}>
          {busy ? "Saving…" : "Save"}
        </button>
      </div>
    </section>
  );
}
