"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCallback, useEffect, useState } from "react";
import { UNAUTHORIZED_EVENT, get, post } from "../lib/api";
import type { AuthStatus, HealthIssue } from "../lib/types";
import { type Key, t } from "../lib/i18n";
import { useLive } from "../lib/live";
import { type Prefs, applyPrefs, remembered } from "../lib/prefs";
import { AuthScreen } from "./AuthScreen";
import { ToastProvider } from "./toast";

const NAV: { group: string; items: { href: string; label: Key }[] }[] = [
  {
    group: "Library",
    items: [
      { href: "/discover/", label: "nav.discover" },
      { href: "/", label: "nav.movies" },
      { href: "/series/", label: "nav.series" },
      { href: "/collections/", label: "nav.collections" },
      { href: "/calendar/", label: "nav.calendar" },
    ],
  },
  {
    group: "Get media",
    items: [
      { href: "/add/", label: "nav.addMovie" },
      { href: "/series/add/", label: "nav.addSeries" },
      { href: "/import/", label: "nav.import" },
      { href: "/wanted/", label: "nav.wanted" },
      { href: "/activity/", label: "nav.activity" },
    ],
  },
  {
    group: "Manage",
    items: [
      { href: "/settings/", label: "nav.settings" },
      { href: "/system/", label: "nav.system" },
    ],
  },
];

type Phase = "loading" | "setup" | "login" | "ready" | "offline";

export function Shell({ children }: { children: React.ReactNode }) {
  const [phase, setPhase] = useState<Phase>("loading");
  const pathname = usePathname();
  // Bumped when the preferences change so every page redraws with them.
  const [version, setVersion] = useState(0);

  useEffect(() => {
    applyPrefs(remembered());
  }, []);

  useEffect(() => {
    if (phase !== "ready") return;
    get<Prefs>("/config/ui")
      .then((p) => {
        applyPrefs(p);
        setVersion((v) => v + 1);
      })
      .catch(() => {});
  }, [phase]);

  const check = useCallback(async () => {
    try {
      const s = await get<AuthStatus>("/auth/status");
      setPhase(s.setup_required ? "setup" : s.authenticated ? "ready" : "login");
    } catch {
      setPhase("offline");
    }
  }, []);

  useEffect(() => {
    check();
    const onUnauthorized = () => setPhase((p) => (p === "ready" ? "login" : p));
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, [check]);

  if (phase === "loading") return <div className="center-screen muted">Loading…</div>;
  if (phase === "offline") {
    return (
      <div className="center-screen stack">
        <p>Cannot reach the server.</p>
        <button onClick={check}>Try again</button>
      </div>
    );
  }
  if (phase === "setup" || phase === "login") {
    return <AuthScreen setup={phase === "setup"} onDone={check} />;
  }

  const active = (href: string) => {
    if (href === "/") return pathname === "/" || pathname.startsWith("/movie");
    if (href === "/series/") return pathname.startsWith("/series") && !pathname.startsWith("/series/add");
    return pathname.startsWith(href);
  };

  return (
    <ToastProvider>
      <div className="layout">
        <aside className="sidebar">
          <div className="brand">OpenMediaProject</div>
          <nav className="nav">
            {NAV.map((g) => (
              <div key={g.group}>
                <div className="nav-group">{g.group}</div>
                {g.items.map((n) => (
                  <Link key={n.href} href={n.href} className={active(n.href) ? "active" : ""}>
                    {t(n.label)}
                  </Link>
                ))}
              </div>
            ))}
          </nav>
        </aside>
        <div className="main">
          <header className="topbar">
            <HealthStatus />
            <button
              className="quiet"
              onClick={async () => {
                await post("/auth/logout").catch(() => {});
                check();
              }}
            >
              {t("nav.signOut")}
            </button>
          </header>
          <main className="content" key={version}>
            {children}
          </main>
        </div>
      </div>
    </ToastProvider>
  );
}

/** What needs attention, in the top bar; details are on the System page. */
function HealthStatus() {
  const [issues, setIssues] = useState<HealthIssue[]>([]);
  const pathname = usePathname();
  const [tick, setTick] = useState(0);
  useLive(() => setTick((t) => t + 1), ["tasks"]);
  useEffect(() => {
    let live = true;
    get<HealthIssue[]>("/system/health")
      .then((i) => live && setIssues(i))
      .catch(() => {});
    return () => {
      live = false;
    };
    // Re-check when moving between pages, so fixing a setting clears it.
  }, [pathname, tick]);
  if (issues.length === 0 || pathname.startsWith("/system")) return <span className="status" />;
  const errors = issues.filter((i) => i.level === "error").length;
  return (
    <Link href="/system/" className="status">
      <strong className={errors ? "error" : "warn"}>
        {errors ? `${errors} problem${errors === 1 ? "" : "s"}` : `${issues.length} warning${issues.length === 1 ? "" : "s"}`}
      </strong>
      <span>{issues[0].message}</span>
    </Link>
  );
}
