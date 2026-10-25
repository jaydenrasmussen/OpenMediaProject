"use client";

import { useEffect, useState } from "react";
import { LoadState } from "../components/Page";
import { useToast } from "../components/toast";
import { get, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import type { DecisionConfig } from "../lib/types";

export function Decision() {
  const notify = useToast();
  const loaded = useLoad(() => get<DecisionConfig>("/config/decision"));
  const [cfg, setCfg] = useState<DecisionConfig | null>(null);
  useEffect(() => {
    if (loaded.data) setCfg(loaded.data);
  }, [loaded.data]);
  if (!cfg) return <LoadState loading={loaded.loading} error={loaded.error} hasData={false} />;
  const set = (patch: Partial<DecisionConfig>) => setCfg({ ...cfg, ...patch });
  const num = (k: keyof DecisionConfig, label: string, hint?: string) => (
    <label className="field">
      <span>
        {label}
        {hint ? ` (${hint})` : ""}
      </span>
      <input type="number" min={0} value={Number(cfg[k])} onChange={(e) => set({ [k]: Number(e.target.value) } as Partial<DecisionConfig>)} />
    </label>
  );

  async function save() {
    try {
      setCfg(await put<DecisionConfig>("/config/decision", cfg));
      notify("Saved.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Release selection</h2>
      <div className="form-grid">
        <label className="field">
          <span>Propers and repacks</span>
          <select value={cfg.propers_and_repacks} onChange={(e) => set({ propers_and_repacks: e.target.value as DecisionConfig["propers_and_repacks"] })}>
            <option value="prefer_and_upgrade">Prefer and upgrade</option>
            <option value="do_not_upgrade">Do not upgrade automatically</option>
            <option value="do_not_prefer">Do not prefer</option>
          </select>
        </label>
        {num("minimum_age", "Usenet minimum age", "minutes")}
        {num("retention", "Usenet retention", "days, 0 is unlimited")}
        {num("maximum_size", "Maximum release size", "MB, 0 is unlimited")}
        {num("availability_delay", "Search after availability", "days")}
        {num("minimum_free_space_when_importing", "Keep free after import", "MB")}
      </div>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={cfg.enable_completed_download_handling} onChange={(e) => set({ enable_completed_download_handling: e.target.checked })} /> Import completed downloads
        </label>
        <label className="check">
          <input type="checkbox" checked={cfg.skip_free_space_check_when_importing} onChange={(e) => set({ skip_free_space_check_when_importing: e.target.checked })} /> Skip the free space check
        </label>
        <label className="check">
          <input type="checkbox" checked={cfg.prefer_indexer_flags} onChange={(e) => set({ prefer_indexer_flags: e.target.checked })} /> Prefer indexer flags
        </label>
        <label className="check">
          <input type="checkbox" checked={cfg.allow_hardcoded_subs} onChange={(e) => set({ allow_hardcoded_subs: e.target.checked })} /> Allow hardcoded subtitles
        </label>
      </div>
      {cfg.allow_hardcoded_subs && (
        <label className="field">
          <span>Hardcoded subtitle languages to allow (comma separated)</span>
          <input type="text" value={cfg.whitelisted_hardcoded_subs} onChange={(e) => set({ whitelisted_hardcoded_subs: e.target.value })} />
        </label>
      )}
      <div className="row end">
        <button className="primary" onClick={save}>
          Save
        </button>
      </div>
    </section>
  );
}
