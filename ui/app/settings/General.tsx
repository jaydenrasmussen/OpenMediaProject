"use client";

import { useState } from "react";
import { useToast } from "../components/toast";
import { get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";
import { Network } from "./Network";

export function General() {
  return (
    <div className="stack">
      <Network />
      <ApiKey />
      <Password />
      <Security />
    </div>
  );
}

function ApiKey() {
  const notify = useToast();
  const [key, setKey] = useState<string | null>(null);

  async function reveal() {
    try {
      setKey((await get<{ api_key: string }>("/auth/apikey")).api_key);
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  async function regenerate() {
    if (!confirm("Programs using the current key will stop working. Continue?")) return;
    try {
      setKey((await post<{ api_key: string }>("/auth/apikey/regenerate")).api_key);
      notify("New API key created.", "good");
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>API key</h2>
      <p className="muted small">
        Programs send this as an <code>X-Api-Key</code> header. It gives full access, so keep it private.
      </p>
      {key ? <code style={{ wordBreak: "break-all" }}>{key}</code> : null}
      <div className="row">
        <button onClick={reveal}>{key ? "Refresh" : "Show"}</button>
        <button className="danger" onClick={regenerate}>
          Generate a new key
        </button>
      </div>
    </section>
  );
}

function Password() {
  const notify = useToast();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);

  async function change(e: React.FormEvent) {
    e.preventDefault();
    if (next !== again) {
      notify("The new passwords do not match.", "bad");
      return;
    }
    setBusy(true);
    try {
      await post("/auth/password", { current_password: current, new_password: next });
      notify("Password changed. Other sessions were signed out.", "good");
      setCurrent("");
      setNext("");
      setAgain("");
    } catch (err) {
      notify(errorMessage(err), "bad");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="card stack" onSubmit={change}>
      <h2 style={{ margin: 0 }}>Password</h2>
      <div className="form-grid">
        <label className="field">
          <span>Current password</span>
          <input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} required />
        </label>
        <label className="field">
          <span>New password (at least 8 characters)</span>
          <input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} required />
        </label>
        <label className="field">
          <span>Repeat new password</span>
          <input type="password" autoComplete="new-password" value={again} onChange={(e) => setAgain(e.target.value)} required />
        </label>
      </div>
      <div className="row end">
        <button className="primary" disabled={busy}>
          Change password
        </button>
      </div>
    </form>
  );
}

function Security() {
  const notify = useToast();
  const state = useLoad(() => get<{ disable_auth_for_local: boolean }>("/config/security"));
  async function set(on: boolean) {
    try {
      await put("/config/security", { disable_auth_for_local: on });
      state.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }
  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>Signing in</h2>
      <label className="check">
        <input type="checkbox" checked={state.data?.disable_auth_for_local ?? false} onChange={(e) => set(e.target.checked)} /> Don’t ask machines on my local network to sign in
      </label>
      <p className="muted small">
        Applies to addresses like 192.168.x.x and 10.x.x.x. It never applies to requests that came through a reverse proxy, so leave this off if you reach the app only through one.
      </p>
    </section>
  );
}
