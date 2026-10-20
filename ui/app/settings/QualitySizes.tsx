"use client";

import { useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { get, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { QualityDefinition } from "../lib/types";

export function QualitySizes() {
  const notify = useToast();
  const defs = useLoad(() => get<QualityDefinition[]>("/qualitydefinition"));
  const [edits, setEdits] = useState<Record<number, Partial<QualityDefinition>>>({});

  const value = (d: QualityDefinition, k: "min_size" | "max_size" | "preferred_size") => {
    const v = edits[d.quality_id]?.[k] ?? d[k];
    return v === null || v === undefined ? "" : String(v);
  };
  const edit = (d: QualityDefinition, k: "min_size" | "max_size" | "preferred_size", raw: string) =>
    setEdits({ ...edits, [d.quality_id]: { ...edits[d.quality_id], [k]: raw === "" ? null : Number(raw) } });

  async function save(d: QualityDefinition) {
    const e = edits[d.quality_id];
    if (!e) return;
    try {
      await put(`/qualitydefinition/${d.quality_id}`, { ...d, ...e });
      const { [d.quality_id]: _gone, ...rest } = edits;
      setEdits(rest);
      defs.reload();
      notify("Saved.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Quality sizes</h2>
      <p className="muted small">Megabytes per minute of runtime. Leave the maximum empty or 0 for no limit.</p>
      <LoadState loading={defs.loading} error={defs.error} hasData={!!defs.data} />
      <div style={{ overflowX: "auto" }}>
        <table>
          <thead>
            <tr>
              <th>Quality</th>
              <th>Min</th>
              <th>Preferred</th>
              <th>Max</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {defs.data?.map((d) => (
              <tr key={d.quality_id}>
                <td>{d.title}</td>
                {(["min_size", "preferred_size", "max_size"] as const).map((k) => (
                  <td key={k}>
                    <input type="number" min={0} step="any" style={{ width: "6rem" }} value={value(d, k)} onChange={(e) => edit(d, k, e.target.value)} aria-label={`${d.title} ${k}`} />
                  </td>
                ))}
                <td>
                  <button disabled={!edits[d.quality_id]} onClick={() => save(d)}>
                    Save
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
