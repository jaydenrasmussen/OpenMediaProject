"use client";

import { useState } from "react";
import { post } from "../lib/api";
import { errorMessage } from "../lib/hooks";

export function AuthScreen({ setup, onDone }: { setup: boolean; onDone: () => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (setup && password !== confirm) {
      setError("The passwords do not match.");
      return;
    }
    setBusy(true);
    try {
      await post(setup ? "/auth/setup" : "/auth/login", { username, password });
      onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="center-screen">
      <form className="card auth-card stack" onSubmit={submit}>
        <h1>{setup ? "Welcome" : "Sign in"}</h1>
        {setup && (
          <p className="muted">
            Create the account that protects this instance. You can change the password later in
            Settings.
          </p>
        )}
        <label className="field">
          <span>Username</span>
          <input
            type="text"
            autoComplete="username"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoFocus
            required
          />
        </label>
        <label className="field">
          <span>Password{setup ? " (at least 8 characters)" : ""}</span>
          <input
            type="password"
            autoComplete={setup ? "new-password" : "current-password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
        </label>
        {setup && (
          <label className="field">
            <span>Repeat password</span>
            <input
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              required
            />
          </label>
        )}
        {error && <p className="error">{error}</p>}
        <button className="primary" disabled={busy}>
          {setup ? "Create account" : "Sign in"}
        </button>
      </form>
    </div>
  );
}
