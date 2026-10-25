"use client";

import type { Schema, SchemaProperty } from "../lib/types";

type Values = Record<string, unknown>;

/** The settings a plugin starts with: the schema's defaults. */
export function schemaDefaults(schema: Schema): Values {
  const out: Values = {};
  for (const [key, p] of Object.entries(schema.properties ?? {})) {
    if (p.default !== undefined) out[key] = p.default;
  }
  return out;
}

function Field({ name, p, required, value, onChange }: { name: string; p: SchemaProperty; required: boolean; value: unknown; onChange: (v: unknown) => void }) {
  const label = p.title ?? name;
  if (p.type === "boolean") {
    return (
      <label className="check">
        <input type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} /> {label}
      </label>
    );
  }
  return (
    <label className="field">
      <span>
        {label}
        {required ? " *" : ""}
      </span>
      {p.enum ? (
        <select value={String(value ?? "")} onChange={(e) => onChange(p.type === "integer" || p.type === "number" ? Number(e.target.value) : e.target.value)}>
          {!required && <option value="" />}
          {p.enum.map((o) => (
            <option key={String(o)} value={String(o)}>
              {String(o)}
            </option>
          ))}
        </select>
      ) : p.type === "integer" || p.type === "number" ? (
        <input type="number" value={value === undefined || value === null ? "" : Number(value)} onChange={(e) => onChange(e.target.value === "" ? undefined : Number(e.target.value))} />
      ) : (
        <input
          type={p.format === "password" ? "password" : "text"}
          value={String(value ?? "")}
          autoComplete="off"
          onChange={(e) => onChange(e.target.value === "" ? undefined : e.target.value)}
        />
      )}
      {p.description && <small className="muted">{p.description}</small>}
    </label>
  );
}

/** Renders a plugin's settings form from its JSON Schema. */
export function SchemaForm({ schema, value, onChange }: { schema: Schema; value: Values; onChange: (v: Values) => void }) {
  const required = new Set(schema.required ?? []);
  return (
    <div className="form-grid">
      {Object.entries(schema.properties ?? {}).map(([name, p]) => (
        <Field
          key={name}
          name={name}
          p={p}
          required={required.has(name)}
          value={value[name]}
          onChange={(v) => {
            const next = { ...value };
            if (v === undefined) delete next[name];
            else next[name] = v;
            onChange(next);
          }}
        />
      ))}
    </div>
  );
}
