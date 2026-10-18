"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useCallback, useEffect, useState } from "react";
import { UNAUTHORIZED_EVENT, get, post } from "../lib/api";
import type { AuthStatus } from "../lib/types";
import { AuthScreen } from "./AuthScreen";
import { ToastProvider } from "./toast";

const NAV = [
  { href: "/", label: "Movies" },
  { href: "/add/", label: "Add movie" },
  { href: "/activity/", label: "Activity" },
  { href: "/settings/", label: "Settings" },
];

type Phase = "loading" | "setup" | "login" | "ready" | "offline";

export function Shell({ children }: { children: React.ReactNode }) {
  const [phase, setPhase] = useState<Phase>("loading");
  const pathname = usePathname();

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

  const active = (href: string) =>
    href === "/" ? pathname === "/" || pathname.startsWith("/movie") : pathname.startsWith(href);

  return (
    <ToastProvider>
      <div className="layout">
        <aside className="sidebar">
          <div className="brand">OpenMediaProject</div>
          <nav className="nav">
            {NAV.map((n) => (
              <Link key={n.href} href={n.href} className={active(n.href) ? "active" : ""}>
                {n.label}
              </Link>
            ))}
            <a
              href="#logout"
              onClick={async (e) => {
                e.preventDefault();
                await post("/auth/logout").catch(() => {});
                check();
              }}
            >
              Sign out
            </a>
          </nav>
        </aside>
        <main className="content">{children}</main>
      </div>
    </ToastProvider>
  );
}
