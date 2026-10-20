"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { DelayProfile, ReleaseProfile, Tag } from "../lib/types";

export function Rules() {
  const tags = useLoad(() => get<Tag[]>("/tag"));
  return (
    <div className="stack">
      <Tags tags={tags} />
      <Delay tags={tags.data ?? []} />
      <Release tags={tags.data ?? []} />
    </div>
  );
}

function Tags({ tags }: { tags: ReturnType<typeof useLoad<Tag[]>> }) {
  const notify = useToast();
  const [label, setLabel] = useState("");
  async function add(e: React.FormEvent) {
    e.preventDefault();
    try {
      await post("/tag", { label: label.trim() });
      setLabel("");
      tags.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function remove(t: Tag) {
    if (!confirm(`Delete the tag “${t.label}”? It is removed from everything that uses it.`)) return;
    try {
      await del(`/tag/${t.id}`);
      tags.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Tags</h2>
      <p className="muted small">Tags tie movies to indexers, download clients, delay profiles and release profiles.</p>
      <LoadState loading={tags.loading} error={tags.error} hasData={!!tags.data} />
      <div className="row">
        {tags.data?.map((t) => (
          <span key={t.id} className="badge">
            {t.label}{" "}
            <button aria-label={`Delete ${t.label}`} onClick={() => remove(t)} style={{ padding: 0, border: 0, background: "none" }}>
              ×
            </button>
          </span>
        ))}
      </div>
      <form className="row" onSubmit={add}>
        <input type="text" placeholder="new tag" value={label} onChange={(e) => setLabel(e.target.value)} aria-label="Tag name" />
        <button className="primary" disabled={!label.trim()}>
          Add tag
        </button>
      </form>
    </section>
  );
}

function TagPicker({ all, value, onChange }: { all: Tag[]; value: number[]; onChange: (v: number[]) => void }) {
  if (all.length === 0) return <p className="muted small">No tags yet. With none selected this applies to everything.</p>;
  return (
    <div className="row">
      {all.map((t) => (
        <label key={t.id} className="check">
          <input
            type="checkbox"
            checked={value.includes(t.id)}
            onChange={(e) => onChange(e.target.checked ? [...value, t.id] : value.filter((v) => v !== t.id))}
          />{" "}
          {t.label}
        </label>
      ))}
    </div>
  );
}

const NEW_DELAY: DelayProfile = {
  id: 0,
  enable_usenet: true,
  enable_torrent: true,
  preferred_protocol: "usenet",
  usenet_delay: 0,
  torrent_delay: 0,
  order: 1,
  bypass_if_highest_quality: false,
  bypass_if_above_custom_format_score: false,
  minimum_custom_format_score: 0,
  tags: [],
};

function Delay({ tags }: { tags: Tag[] }) {
  const notify = useToast();
  const list = useLoad(() => get<DelayProfile[]>("/delayprofile"));
  const [draft, setDraft] = useState<DelayProfile | null>(null);
  const names = (ids: number[]) => (ids.length ? ids.map((i) => tags.find((t) => t.id === i)?.label ?? `#${i}`).join(", ") : "all movies");

  async function save() {
    if (!draft) return;
    try {
      if (draft.id) await put(`/delayprofile/${draft.id}`, draft);
      else await post("/delayprofile", draft);
      setDraft(null);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function remove(p: DelayProfile) {
    if (!confirm("Delete this delay profile?")) return;
    try {
      await del(`/delayprofile/${p.id}`);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  const set = (patch: Partial<DelayProfile>) => setDraft({ ...draft!, ...patch });

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Delay profiles</h2>
      <p className="muted small">Wait for a better release before grabbing. The first matching profile (lowest order) applies.</p>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data?.map((p) => (
        <div key={p.id} className="spread">
          <span>
            <strong>
              {p.enable_usenet ? `Usenet ${p.usenet_delay} min` : "Usenet off"} · {p.enable_torrent ? `Torrent ${p.torrent_delay} min` : "Torrent off"}
            </strong>{" "}
            <span className="muted small">
              prefers {p.preferred_protocol} · for {names(p.tags)}
            </span>
          </span>
          <span className="row">
            <button onClick={() => setDraft(structuredClone(p))}>Edit</button>
            {p.tags.length > 0 && (
              <button className="danger" onClick={() => remove(p)}>
                Delete
              </button>
            )}
          </span>
        </div>
      ))}
      {draft ? (
        <div className="stack">
          <div className="form-grid">
            <label className="field">
              <span>Preferred protocol</span>
              <select value={draft.preferred_protocol} onChange={(e) => set({ preferred_protocol: e.target.value as DelayProfile["preferred_protocol"] })}>
                <option value="usenet">Usenet</option>
                <option value="torrent">Torrent</option>
              </select>
            </label>
            <label className="field">
              <span>Usenet delay (minutes)</span>
              <input type="number" min={0} value={draft.usenet_delay} onChange={(e) => set({ usenet_delay: Number(e.target.value) })} />
            </label>
            <label className="field">
              <span>Torrent delay (minutes)</span>
              <input type="number" min={0} value={draft.torrent_delay} onChange={(e) => set({ torrent_delay: Number(e.target.value) })} />
            </label>
            <label className="field">
              <span>Order</span>
              <input type="number" value={draft.order} onChange={(e) => set({ order: Number(e.target.value) })} />
            </label>
            <label className="field">
              <span>Bypass at custom format score</span>
              <input type="number" value={draft.minimum_custom_format_score} onChange={(e) => set({ minimum_custom_format_score: Number(e.target.value) })} />
            </label>
          </div>
          <div className="row">
            <label className="check">
              <input type="checkbox" checked={draft.enable_usenet} onChange={(e) => set({ enable_usenet: e.target.checked })} /> Usenet
            </label>
            <label className="check">
              <input type="checkbox" checked={draft.enable_torrent} onChange={(e) => set({ enable_torrent: e.target.checked })} /> Torrents
            </label>
            <label className="check">
              <input type="checkbox" checked={draft.bypass_if_highest_quality} onChange={(e) => set({ bypass_if_highest_quality: e.target.checked })} /> Skip the wait for the best quality
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={draft.bypass_if_above_custom_format_score}
                onChange={(e) => set({ bypass_if_above_custom_format_score: e.target.checked })}
              />{" "}
              Skip the wait above the score
            </label>
          </div>
          <TagPicker all={tags} value={draft.tags} onChange={(v) => set({ tags: v })} />
          <div className="row end">
            <button onClick={() => setDraft(null)}>Cancel</button>
            <button className="primary" onClick={save}>
              Save
            </button>
          </div>
        </div>
      ) : (
        <div className="row end">
          <button onClick={() => setDraft({ ...NEW_DELAY, order: (list.data?.length ?? 0) + 1 })}>Add delay profile</button>
        </div>
      )}
    </section>
  );
}

const NEW_RELEASE: ReleaseProfile = { id: 0, name: "", enabled: true, required: [], ignored: [], indexer_id: 0, tags: [] };
const lines = (s: string) => s.split("\n").map((l) => l.trim()).filter(Boolean);

function Release({ tags }: { tags: Tag[] }) {
  const notify = useToast();
  const list = useLoad(() => get<ReleaseProfile[]>("/releaseprofile"));
  const [draft, setDraft] = useState<ReleaseProfile | null>(null);
  const set = (patch: Partial<ReleaseProfile>) => setDraft({ ...draft!, ...patch });

  async function save() {
    if (!draft) return;
    try {
      if (draft.id) await put(`/releaseprofile/${draft.id}`, draft);
      else await post("/releaseprofile", draft);
      setDraft(null);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function remove(p: ReleaseProfile) {
    if (!confirm(`Delete “${p.name}”?`)) return;
    try {
      await del(`/releaseprofile/${p.id}`);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Release profiles</h2>
      <p className="muted small">Terms a release must contain, or must not. Terms wrapped in slashes, like /\bremux\b/, are regular expressions.</p>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {list.data?.map((p) => (
        <div key={p.id} className="spread">
          <span>
            <strong>{p.name}</strong> {!p.enabled && <span className="badge">off</span>}{" "}
            <span className="muted small">
              {p.required.length} required · {p.ignored.length} ignored
            </span>
          </span>
          <span className="row">
            <button onClick={() => setDraft(structuredClone(p))}>Edit</button>
            <button className="danger" onClick={() => remove(p)}>
              Delete
            </button>
          </span>
        </div>
      ))}
      {draft ? (
        <div className="stack">
          <label className="field">
            <span>Name</span>
            <input type="text" value={draft.name} onChange={(e) => set({ name: e.target.value })} />
          </label>
          <label className="field">
            <span>Must contain at least one of (one per line)</span>
            <textarea rows={3} defaultValue={draft.required.join("\n")} onChange={(e) => set({ required: lines(e.target.value) })} style={{ width: "100%" }} />
          </label>
          <label className="field">
            <span>Must not contain (one per line)</span>
            <textarea rows={3} defaultValue={draft.ignored.join("\n")} onChange={(e) => set({ ignored: lines(e.target.value) })} style={{ width: "100%" }} />
          </label>
          <label className="check">
            <input type="checkbox" checked={draft.enabled} onChange={(e) => set({ enabled: e.target.checked })} /> Enabled
          </label>
          <TagPicker all={tags} value={draft.tags} onChange={(v) => set({ tags: v })} />
          <div className="row end">
            <button onClick={() => setDraft(null)}>Cancel</button>
            <button className="primary" onClick={save} disabled={!draft.name.trim()}>
              Save
            </button>
          </div>
        </div>
      ) : (
        <div className="row end">
          <button onClick={() => setDraft({ ...NEW_RELEASE })}>Add release profile</button>
        </div>
      )}
    </section>
  );
}
