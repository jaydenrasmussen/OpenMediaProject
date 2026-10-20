"use client";

import { useEffect, useRef } from "react";

// One WebSocket for the whole page. The server only says *what kind* of thing
// changed; screens listening for that kind reload themselves.

type Listener = (kind: string) => void;

const listeners = new Set<Listener>();
let socket: WebSocket | null = null;
let retry = 1000;
let timer: ReturnType<typeof setTimeout> | null = null;

function connect() {
  if (socket || typeof window === "undefined") return;
  const base = process.env.NEXT_PUBLIC_API_BASE;
  const url = base ? base.replace(/^http/, "ws") + "/api/v1/ws" : `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/api/v1/ws`;
  try {
    socket = new WebSocket(url);
  } catch {
    socket = null;
    return;
  }
  socket.onopen = () => {
    retry = 1000;
  };
  socket.onmessage = (m) => {
    try {
      const { kind } = JSON.parse(String(m.data)) as { kind: string };
      listeners.forEach((l) => l(kind));
    } catch {
      // Not ours; ignore.
    }
  };
  socket.onclose = () => {
    socket = null;
    // Everything may have changed while we were away.
    if (listeners.size > 0) {
      listeners.forEach((l) => l("changed"));
      timer = setTimeout(connect, retry);
      retry = Math.min(retry * 2, 30000);
    }
  };
  socket.onerror = () => socket?.close();
}

function subscribe(l: Listener): () => void {
  listeners.add(l);
  connect();
  return () => {
    listeners.delete(l);
    if (listeners.size === 0) {
      if (timer) clearTimeout(timer);
      socket?.close();
      socket = null;
    }
  };
}

/** Calls `reload` (debounced) when the server reports one of `kinds`. "changed" always counts. */
export function useLive(reload: () => void, kinds: string[]) {
  const ref = useRef(reload);
  ref.current = reload;
  const wanted = kinds.join(",");
  useEffect(() => {
    const set = new Set(wanted.split(","));
    let pending: ReturnType<typeof setTimeout> | null = null;
    const off = subscribe((kind) => {
      if (kind !== "changed" && !set.has(kind)) return;
      if (pending) return;
      pending = setTimeout(() => {
        pending = null;
        if (document.visibilityState === "visible") ref.current();
      }, 300);
    });
    return () => {
      off();
      if (pending) clearTimeout(pending);
    };
  }, [wanted]);
}
