"use client";

import { useEffect, useMemo } from "react";
import { SchemaForm, schemaDefaults } from "../components/SchemaForm";
import { get } from "../lib/api";
import { useLoad } from "../lib/hooks";
import type { PluginList, Schema } from "../lib/types";
import type { Implementation } from "./ProviderEditor";

/** The settings form of one plugin, drawn from its JSON Schema. */
function PluginFields({ pluginId, settings, onChange }: { pluginId: string; settings: Record<string, unknown>; onChange: (s: Record<string, unknown>) => void }) {
  const schema = useLoad(() => get<Schema>(`/plugin/${pluginId}/schema`), [pluginId]);
  const loaded = schema.data;
  useEffect(() => {
    // A new provider starts from the plugin's defaults.
    if (loaded && Object.keys(settings).length === 0) onChange(schemaDefaults(loaded));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded]);
  if (schema.error) return <p className="error">{schema.error}</p>;
  if (!loaded) return <p className="muted">Loading…</p>;
  return <SchemaForm schema={loaded} value={settings} onChange={onChange} />;
}

/** Implementations contributed by installed plugins of one kind. */
export function usePluginImplementations(kind: "indexer" | "list"): Implementation[] {
  const plugins = useLoad(() => get<PluginList>("/plugin"));
  return useMemo(
    () =>
      (plugins.data?.plugins ?? [])
        .filter((p) => p.kind === kind)
        .map((p) => ({
          id: p.id,
          label: `${p.name} (plugin)`,
          pluginId: p.id,
          defaults: {},
          describe: () => p.description,
          Fields: ({ settings, onChange }) => <PluginFields pluginId={p.id} settings={settings} onChange={onChange} />,
        })),
    [plugins.data, kind],
  );
}
