"use client";

import { useEffect, useState } from "react";
import { useToast } from "../components/toast";
import { get, post, put } from "../lib/api";
import { errorMessage, useLoad } from "../lib/hooks";

interface Settings {
  provider: string;
  vpn_type: string;
  username: string;
  password: string;
  wireguard_private_key: string;
  wireguard_addresses: string;
  server_countries: string;
  server_cities: string;
  server_hostnames: string;
  firewall_outbound_subnets: string;
}

interface Loaded {
  managed_by_environment: boolean;
  providers: string[];
  settings: Omit<Settings, "password" | "wireguard_private_key">;
  password_set: boolean;
  wireguard_private_key_set: boolean;
}

const EMPTY: Settings = {
  provider: "",
  vpn_type: "",
  username: "",
  password: "",
  wireguard_private_key: "",
  wireguard_addresses: "",
  server_countries: "",
  server_cities: "",
  server_hostnames: "",
  firewall_outbound_subnets: "",
};

/** What each provider asks for, in the words its own account page uses. */
const HINTS: Record<string, string> = {
  mullvad: "OpenVPN: your account number is the user name and the password can be left empty. WireGuard: paste the key and the address Mullvad gave you.",
  nordvpn: "Use the service credentials from the Nord account dashboard, not your login. For WireGuard, paste the private key.",
  "private internet access": "Use your Private Internet Access user name and password.",
  protonvpn: "Use the OpenVPN / IKEv2 user name and password from your Proton account, not your login.",
  surfshark: "Use the service credentials from the Surfshark manual setup page.",
  custom: "Gluetun reads your own OpenVPN or WireGuard file from /gluetun inside the container; see the VPN docs.",
};

export function Vpn() {
  const notify = useToast();
  const loaded = useLoad(() => get<Loaded>("/system/vpn/settings"));
  const [s, setS] = useState<Settings>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (loaded.data) setS({ ...EMPTY, ...loaded.data.settings });
  }, [loaded.data]);

  if (!loaded.data) return <section className="card muted">Loading…</section>;
  const d = loaded.data;
  const locked = d.managed_by_environment;
  const set = (k: keyof Settings, v: string) => setS({ ...s, [k]: v });
  const wireguard = s.vpn_type === "wireguard";
  const on = s.provider !== "";

  async function save() {
    setBusy(true);
    try {
      await put("/system/vpn/settings", s);
      setSaved(true);
      notify(on ? "Saved. Restart for the VPN to start." : "Saved. Restart for the VPN to stop.", "good");
      loaded.reload();
    } catch (e) {
      notify(errorMessage(e), "bad");
    } finally {
      setBusy(false);
    }
  }

  async function restart() {
    if (!confirm("Restart now? The page is unavailable for a few seconds.")) return;
    try {
      await post("/system/restart");
      notify("Restarting…");
    } catch (e) {
      notify(errorMessage(e), "bad");
    }
  }

  return (
    <section className="card stack">
      <h2 style={{ margin: 0 }}>VPN</h2>
      <p className="muted small">
        Everything this app does on the internet (searching, downloading, looking up posters) can go through a VPN. If the VPN drops, traffic stops instead of using your own address. The container needs <code>--cap-add NET_ADMIN --device /dev/net/tun</code> and a restart policy so it comes back after a restart.
      </p>
      {locked && (
        <div className="banner warn">The VPN is set by this container’s environment variables, so it can’t be changed here.</div>
      )}
      <div className="form-grid">
        <label className="field">
          <span>Provider</span>
          <select value={s.provider} disabled={locked} onChange={(e) => set("provider", e.target.value)}>
            <option value="">No VPN</option>
            {d.providers.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
        {on && (
          <label className="field">
            <span>Connection</span>
            <select value={s.vpn_type} disabled={locked} onChange={(e) => set("vpn_type", e.target.value)}>
              <option value="">Provider’s default</option>
              <option value="openvpn">OpenVPN</option>
              <option value="wireguard">WireGuard</option>
            </select>
          </label>
        )}
      </div>
      {on && HINTS[s.provider] && <p className="muted small">{HINTS[s.provider]}</p>}
      {on && !wireguard && (
        <div className="form-grid">
          <label className="field">
            <span>User name</span>
            <input type="text" autoComplete="off" disabled={locked} value={s.username} onChange={(e) => set("username", e.target.value)} />
          </label>
          <label className="field">
            <span>Password</span>
            <input
              type="password"
              autoComplete="new-password"
              disabled={locked}
              value={s.password}
              placeholder={d.password_set ? "Saved. Type to replace." : ""}
              onChange={(e) => set("password", e.target.value)}
            />
          </label>
        </div>
      )}
      {on && wireguard && (
        <div className="form-grid">
          <label className="field">
            <span>Private key</span>
            <input
              type="password"
              autoComplete="new-password"
              disabled={locked}
              value={s.wireguard_private_key}
              placeholder={d.wireguard_private_key_set ? "Saved. Type to replace." : ""}
              onChange={(e) => set("wireguard_private_key", e.target.value)}
            />
          </label>
          <label className="field">
            <span>Address (if your provider gives one)</span>
            <input type="text" placeholder="10.64.222.21/32" disabled={locked} value={s.wireguard_addresses} onChange={(e) => set("wireguard_addresses", e.target.value)} />
          </label>
        </div>
      )}
      {on && (
        <>
          <div className="form-grid">
            <label className="field">
              <span>Countries (optional)</span>
              <input type="text" placeholder="Sweden, Norway" disabled={locked} value={s.server_countries} onChange={(e) => set("server_countries", e.target.value)} />
            </label>
            <label className="field">
              <span>Cities (optional)</span>
              <input type="text" placeholder="Stockholm" disabled={locked} value={s.server_cities} onChange={(e) => set("server_cities", e.target.value)} />
            </label>
            <label className="field">
              <span>Servers (optional)</span>
              <input type="text" placeholder="se-sto-wg-001" disabled={locked} value={s.server_hostnames} onChange={(e) => set("server_hostnames", e.target.value)} />
            </label>
          </div>
          <label className="field">
            <span>Local networks the app may still reach</span>
            <input
              type="text"
              placeholder="192.168.1.0/24"
              disabled={locked}
              value={s.firewall_outbound_subnets}
              onChange={(e) => set("firewall_outbound_subnets", e.target.value)}
            />
            <span className="muted small">Needed to reach a Plex, Jellyfin or download client on your own network, because the VPN blocks it otherwise.</span>
          </label>
        </>
      )}
      <div className="row end">
        {saved && <button onClick={restart}>Restart now</button>}
        <button className="primary" onClick={save} disabled={busy || locked}>
          Save
        </button>
      </div>
    </section>
  );
}
