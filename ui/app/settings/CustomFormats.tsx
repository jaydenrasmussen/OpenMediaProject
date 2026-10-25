"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { del, get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { Condition, CustomFormat, Specification } from "../lib/types";

type Impl = Condition["implementation"];

const IMPLS: { id: Impl; label: string; kind: "text" | "number" | "range"; hint?: string }[] = [
  { id: "release_title", label: "Release title (regex)", kind: "text" },
  { id: "edition", label: "Edition (regex)", kind: "text" },
  { id: "release_group", label: "Release group (regex)", kind: "text" },
  { id: "language", label: "Language", kind: "number", hint: "Radarr language id" },
  { id: "indexer_flag", label: "Indexer flag", kind: "number", hint: "bit value, e.g. 1 freeleech, 32 internal" },
  { id: "source", label: "Source", kind: "number", hint: "Radarr source id" },
  { id: "resolution", label: "Resolution", kind: "number", hint: "360, 480, 540, 576, 720, 1080, 2160" },
  { id: "quality_modifier", label: "Quality modifier", kind: "number", hint: "Radarr modifier id" },
  { id: "size", label: "Size (GB)", kind: "range" },
  { id: "year", label: "Year", kind: "range" },
];

function blank(impl: Impl): Condition {
  switch (impl) {
    case "release_title":
    case "edition":
    case "release_group":
      return { implementation: impl, value: "" };
    case "language":
      return { implementation: impl, value: 1, except_language: false };
    case "size":
      return { implementation: impl, min: 0, max: 10 };
    case "year":
      return { implementation: impl, min: 1900, max: 2100 };
    default:
      return { implementation: impl, value: 0 };
  }
}

const NEW: CustomFormat = { id: 0, name: "", include_when_renaming: false, specifications: [] };

export function CustomFormats() {
  const notify = useToast();
  const list = useLoad(() => get<CustomFormat[]>("/customformat"));
  const [draft, setDraft] = useState<CustomFormat | null>(null);

  async function save() {
    if (!draft) return;
    try {
      if (draft.id) await put(`/customformat/${draft.id}`, draft);
      else await post("/customformat", draft);
      setDraft(null);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  async function remove(f: CustomFormat) {
    if (!confirm(`Delete “${f.name}”? Quality profile scores for it go too.`)) return;
    try {
      await del(`/customformat/${f.id}`);
      list.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }
  const setSpec = (i: number, spec: Specification) =>
    setDraft({ ...draft!, specifications: draft!.specifications.map((s, j) => (j === i ? spec : s)) });

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Custom formats</h2>
      <p className="muted small">
        A release matches a format when every <em>required</em> condition holds and at least one other condition does (a condition can be negated).
      </p>
      <LoadState loading={list.loading} error={list.error} hasData={!!list.data} />
      {!draft &&
        list.data?.map((f) => (
          <div key={f.id} className="spread">
            <span>
              <strong>{f.name}</strong> <span className="muted small">{f.specifications.length} conditions</span>
            </span>
            <span className="row">
              <button onClick={() => setDraft(structuredClone(f))}>Edit</button>
              <button className="danger" onClick={() => remove(f)}>
                Delete
              </button>
            </span>
          </div>
        ))}
      {draft ? (
        <div className="stack">
          <label className="field">
            <span>Name</span>
            <input type="text" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </label>
          <label className="check">
            <input type="checkbox" checked={draft.include_when_renaming} onChange={(e) => setDraft({ ...draft, include_when_renaming: e.target.checked })} /> Include in the{" "}
            <code>{"{Custom Formats}"}</code> naming token
          </label>
          {draft.specifications.map((s, i) => {
            const impl = IMPLS.find((x) => x.id === s.implementation)!;
            return (
              <div key={i} className="card stack">
                <div className="form-grid">
                  <label className="field">
                    <span>Condition</span>
                    <select
                      value={s.implementation}
                      onChange={(e) => setSpec(i, { name: s.name, negate: s.negate, required: s.required, ...blank(e.target.value as Impl) })}
                    >
                      {IMPLS.map((x) => (
                        <option key={x.id} value={x.id}>
                          {x.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="field">
                    <span>Label</span>
                    <input type="text" value={s.name} onChange={(e) => setSpec(i, { ...s, name: e.target.value })} />
                  </label>
                  {impl.kind === "text" && "value" in s && (
                    <label className="field">
                      <span>Pattern</span>
                      <input type="text" value={String(s.value)} onChange={(e) => setSpec(i, { ...s, value: e.target.value } as Specification)} />
                    </label>
                  )}
                  {impl.kind === "number" && "value" in s && (
                    <label className="field">
                      <span>Value{impl.hint ? ` (${impl.hint})` : ""}</span>
                      <input type="number" value={Number(s.value)} onChange={(e) => setSpec(i, { ...s, value: Number(e.target.value) } as Specification)} />
                    </label>
                  )}
                  {impl.kind === "range" && "min" in s && (
                    <>
                      <label className="field">
                        <span>Min</span>
                        <input type="number" value={s.min} onChange={(e) => setSpec(i, { ...s, min: Number(e.target.value) } as Specification)} />
                      </label>
                      <label className="field">
                        <span>Max</span>
                        <input type="number" value={s.max} onChange={(e) => setSpec(i, { ...s, max: Number(e.target.value) } as Specification)} />
                      </label>
                    </>
                  )}
                </div>
                <div className="spread">
                  <span className="row">
                    <label className="check">
                      <input type="checkbox" checked={s.negate} onChange={(e) => setSpec(i, { ...s, negate: e.target.checked })} /> Negate
                    </label>
                    <label className="check">
                      <input type="checkbox" checked={s.required} onChange={(e) => setSpec(i, { ...s, required: e.target.checked })} /> Required
                    </label>
                    {s.implementation === "language" && (
                      <label className="check">
                        <input type="checkbox" checked={s.except_language} onChange={(e) => setSpec(i, { ...s, except_language: e.target.checked })} /> Except this language
                      </label>
                    )}
                  </span>
                  <button className="danger" onClick={() => setDraft({ ...draft, specifications: draft.specifications.filter((_, j) => j !== i) })}>
                    Remove condition
                  </button>
                </div>
              </div>
            );
          })}
          <div className="row">
            <button onClick={() => setDraft({ ...draft, specifications: [...draft.specifications, { name: "", negate: false, required: false, ...blank("release_title") }] })}>
              Add condition
            </button>
          </div>
          <div className="row end">
            <button onClick={() => setDraft(null)}>Cancel</button>
            <button className="primary" onClick={save} disabled={!draft.name.trim() || draft.specifications.length === 0}>
              Save
            </button>
          </div>
        </div>
      ) : (
        <div className="row end">
          <button onClick={() => setDraft(structuredClone(NEW))}>Add custom format</button>
        </div>
      )}
    </section>
  );
}
