"use client";

import type { Tag } from "../lib/types";

/** Check boxes for choosing tags. `empty` is shown when no tags exist yet. */
export function TagPicker({
  all,
  value,
  onChange,
  empty = "No tags yet. With none selected this applies to everything.",
}: {
  all: Tag[];
  value: number[];
  onChange: (v: number[]) => void;
  empty?: string;
}) {
  if (all.length === 0) return <p className="muted small">{empty}</p>;
  return (
    <div className="row">
      {all.map((t) => (
        <label key={t.id} className="check">
          <input type="checkbox" checked={value.includes(t.id)} onChange={(e) => onChange(e.target.checked ? [...value, t.id] : value.filter((v) => v !== t.id))} /> {t.label}
        </label>
      ))}
    </div>
  );
}
