"use client";

import { useEffect, useState } from "react";
import { useToast } from "../components/toast";
import { get, put } from "../lib/api";
import { LANGUAGES } from "../lib/i18n";
import { errorMessage } from "../lib/hooks";
import { DEFAULT_PREFS, type Prefs, applyPrefs, formatDateTime } from "../lib/prefs";

export function Interface() {
  const notify = useToast();
  const [form, setForm] = useState<Prefs>(DEFAULT_PREFS);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    get<Prefs>("/config/ui")
      .then((p) => {
        setForm(p);
        setLoaded(true);
      })
      .catch((e) => notify(errorMessage(e), "bad"));
  }, [notify]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    try {
      const saved = await put<Prefs>("/config/ui", form);
      applyPrefs(saved);
      notify("Saved.", "good");
    } catch (err) {
      notify(errorMessage(err), "bad");
    }
  }

  const set = <K extends keyof Prefs>(key: K, value: Prefs[K]) => setForm((f) => ({ ...f, [key]: value }));
  const sample = formatDateTime(new Date(2025, 0, 31, 18, 5), true);

  return (
    <form className="card stack" onSubmit={save}>
      <h2 style={{ margin: 0 }}>Interface</h2>
      <div className="form-grid">
        <label className="field">
          <span>Language</span>
          <select value={form.language} onChange={(e) => set("language", e.target.value)}>
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Theme</span>
          <select value={form.theme} onChange={(e) => set("theme", e.target.value as Prefs["theme"])}>
            <option value="auto">Follow the system</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
        <label className="field">
          <span>Date format</span>
          <select value={form.date_format} onChange={(e) => set("date_format", e.target.value as Prefs["date_format"])}>
            <option value="locale">Browser default</option>
            <option value="iso">2025-01-31</option>
            <option value="dmy">31/01/2025</option>
            <option value="mdy">01/31/2025</option>
          </select>
        </label>
        <label className="field">
          <span>Time format</span>
          <select value={form.time_format} onChange={(e) => set("time_format", e.target.value as Prefs["time_format"])}>
            <option value="locale">Browser default</option>
            <option value="24h">24 hour</option>
            <option value="12h">12 hour</option>
          </select>
        </label>
      </div>
      <p className="muted small">Saved settings are shared by every browser. They look like this: {sample}</p>
      <div className="row end">
        <button className="primary" disabled={!loaded}>
          Save
        </button>
      </div>
    </form>
  );
}
