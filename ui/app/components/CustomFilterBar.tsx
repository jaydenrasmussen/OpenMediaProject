"use client";

import { useState } from "react";
import { useToast } from "./toast";
import { del, get, post } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { CustomFilter } from "../lib/types";

/**
 * A dropdown of saved filters for one library view (movies, series, ...),
 * plus the ability to save the view's current filter/sort as a new one or
 * delete the one in use. `state` is whatever the view's own filter controls
 * use; it is stored and restored as opaque JSON, so this component does not
 * need to know its shape.
 */
export function CustomFilterBar<S extends Record<string, unknown>>({
  filterType,
  state,
  onApply,
}: {
  filterType: string;
  state: S;
  onApply: (state: S) => void;
}) {
  const notify = useToast();
  const saved = useLoad(() => get<CustomFilter[]>(`/customfilter?type=${filterType}`));
  const [selected, setSelected] = useState<number | "">("");
  const [saving, setSaving] = useState(false);

  function apply(id: number | "") {
    setSelected(id);
    if (id === "") return;
    const found = saved.data?.find((f) => f.id === id);
    if (found?.filters[0]) onApply(found.filters[0] as S);
  }

  async function saveCurrent() {
    const label = prompt("Save the current filter as:");
    if (!label?.trim()) return;
    setSaving(true);
    try {
      const created = await post<CustomFilter>("/customfilter", {
        type: filterType,
        label: label.trim(),
        filters: [state],
      });
      saved.reload();
      setSelected(created.id);
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setSaving(false);
    }
  }

  async function removeSelected() {
    if (selected === "") return;
    const found = saved.data?.find((f) => f.id === selected);
    if (!found || !confirm(`Delete the saved filter “${found.label}”?`)) return;
    try {
      await del(`/customfilter/${selected}`);
      setSelected("");
      saved.reload();
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <div className="row">
      <select
        value={selected}
        onChange={(e) => apply(e.target.value === "" ? "" : Number(e.target.value))}
        aria-label="Saved filters"
      >
        <option value="">Saved filters…</option>
        {saved.data?.map((f) => (
          <option key={f.id} value={f.id}>
            {f.label}
          </option>
        ))}
      </select>
      <button onClick={saveCurrent} disabled={saving} title="Save the current filter and sort">
        Save filter
      </button>
      {selected !== "" && (
        <button className="danger" onClick={removeSelected}>
          Delete
        </button>
      )}
    </div>
  );
}
