"use client";

import { LoadState } from "../components/Page";
import { useState } from "react";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { CustomFormat, ProfileItem, QualityProfile } from "../lib/types";

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
  const notify = useToast();
  const profiles = useLoad(() => get<QualityProfile[]>("/qualityprofile"));
  const formats = useLoad(() => get<CustomFormat[]>("/customformat"));
  const [draft, setDraft] = useState<QualityProfile | null>(null);

  async function save() {
    if (!draft) return;
    try {
      if (draft.id) await put(`/qualityprofile/${draft.id}`, draft);
      else await post("/qualityprofile", draft);
      setDraft(null);
      profiles.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function remove(p: QualityProfile) {
    if (!confirm(`Delete “${p.name}”?`)) return;
    try {
      await del(`/qualityprofile/${p.id}`);
      profiles.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  function copy(p: QualityProfile) {
    setDraft({ ...structuredClone(p), id: 0, name: `${p.name} copy` });
  }

  if (draft) return <Editor draft={draft} setDraft={setDraft} formats={formats.data ?? []} onSave={save} />;

  return (
    <div className="stack">
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
            <div className="row end" style={{ marginTop: ".5rem" }}>
              <button onClick={() => setDraft(structuredClone(p))}>Edit</button>
              <button onClick={() => copy(p)}>Copy</button>
              <button className="danger" onClick={() => remove(p)}>
                Delete
              </button>
            </div>
          </div>
        );
      })}
      <div className="row end">
        <button
          onClick={() => {
            const base = profiles.data?.[0];
            if (base) setDraft({ ...structuredClone(base), id: 0, name: "" });
          }}
          disabled={!profiles.data?.length}
        >
          Add profile
        </button>
      </div>
    </div>
  );
}

function rowId(item: ProfileItem): number {
  return item.Quality ? item.Quality.quality : item.Group!.id;
}
function rowName(item: ProfileItem): string {
  return item.Quality ? (NAMES[item.Quality.quality] ?? `#${item.Quality.quality}`) : item.Group!.name;
}
function rowAllowed(item: ProfileItem): boolean {
  return item.Quality ? item.Quality.allowed : item.Group!.allowed;
}

function Editor({
  draft,
  setDraft,
  formats,
  onSave,
}: {
  draft: QualityProfile;
  setDraft: (p: QualityProfile | null) => void;
  formats: CustomFormat[];
  onSave: () => void;
}) {
  const set = (patch: Partial<QualityProfile>) => setDraft({ ...draft, ...patch });
  const setItems = (items: ProfileItem[]) => set({ items });
  const toggle = (i: number, allowed: boolean) =>
    setItems(
      draft.items.map((it, j) => {
        if (j !== i) return it;
        return it.Quality ? { Quality: { ...it.Quality, allowed } } : { Group: { ...it.Group!, allowed } };
      }),
    );
  const move = (i: number, d: number) => {
    const items = [...draft.items];
    const j = i + d;
    if (j < 0 || j >= items.length) return;
    [items[i], items[j]] = [items[j], items[i]];
    setItems(items);
  };
  const score = (id: number) => draft.format_items.find((f) => f.format_id === id)?.score ?? 0;
  const setScore = (id: number, value: number) => {
    const rest = draft.format_items.filter((f) => f.format_id !== id);
    set({ format_items: value === 0 ? rest : [...rest, { format_id: id, score: value }] });
  };
  const cutoffChoices = draft.items.filter(rowAllowed);

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>{draft.id ? "Edit profile" : "New profile"}</h2>
      <div className="form-grid">
        <label className="field">
          <span>Name</span>
          <input type="text" value={draft.name} onChange={(e) => set({ name: e.target.value })} autoFocus />
        </label>
        <label className="field">
          <span>Upgrade until</span>
          <select value={draft.cutoff} onChange={(e) => set({ cutoff: Number(e.target.value) })}>
            {cutoffChoices.map((it) => (
              <option key={rowId(it)} value={rowId(it)}>
                {rowName(it)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Minimum custom format score</span>
          <input type="number" value={draft.min_format_score} onChange={(e) => set({ min_format_score: Number(e.target.value) })} />
        </label>
        <label className="field">
          <span>Upgrade until score</span>
          <input type="number" value={draft.cutoff_format_score} onChange={(e) => set({ cutoff_format_score: Number(e.target.value) })} />
        </label>
        <label className="field">
          <span>Minimum score improvement</span>
          <input type="number" min={1} value={draft.min_upgrade_format_score} onChange={(e) => set({ min_upgrade_format_score: Number(e.target.value) })} />
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={draft.upgrade_allowed} onChange={(e) => set({ upgrade_allowed: e.target.checked })} /> Allow upgrades once a file exists
      </label>

      <h3>Qualities</h3>
      <p className="muted small">Lowest to highest. Tick the ones you accept.</p>
      {draft.items.map((it, i) => (
        <div key={rowId(it)} className="spread">
          <label className="check">
            <input type="checkbox" checked={rowAllowed(it)} onChange={(e) => toggle(i, e.target.checked)} /> {rowName(it)}
            {it.Group && <span className="muted small"> ({it.Group.qualities.length} qualities)</span>}
          </label>
          <span className="row">
            <button aria-label="Move up the list" onClick={() => move(i, -1)} disabled={i === 0}>
              ↑
            </button>
            <button aria-label="Move down the list" onClick={() => move(i, 1)} disabled={i === draft.items.length - 1}>
              ↓
            </button>
          </span>
        </div>
      ))}

      <h3>Custom format scores</h3>
      {formats.length === 0 && <p className="muted small">No custom formats yet.</p>}
      <div className="form-grid">
        {formats.map((f) => (
          <label key={f.id} className="field">
            <span>{f.name}</span>
            <input type="number" value={score(f.id)} onChange={(e) => setScore(f.id, Number(e.target.value))} />
          </label>
        ))}
      </div>

      <div className="row end">
        <button onClick={() => setDraft(null)}>Cancel</button>
        <button className="primary" onClick={onSave} disabled={!draft.name.trim()}>
          Save
        </button>
      </div>
    </section>
  );
}
