"use client";

/** Counts that double as the filter: each is a peer, and the chosen one is inverted. */
export function Counts<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { id: T; label: string; count: number }[];
  value: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="counts" role="group" aria-label="Show">
      {options.map((o) => (
        <button key={o.id} className={o.id === value ? "on" : ""} aria-pressed={o.id === value} onClick={() => onChange(o.id)}>
          <span>{o.label}</span>
          <strong>{o.count}</strong>
        </button>
      ))}
    </div>
  );
}
