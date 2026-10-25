"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "./api";

export interface Loaded<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
}

/** Runs `fetcher` on mount and whenever `deps` change; `reload()` runs it again. */
export function useLoad<T>(fetcher: () => Promise<T>, deps: unknown[] = []): Loaded<T> {
  const [state, setState] = useState<{ data: T | null; error: string | null; loading: boolean }>({
    data: null,
    error: null,
    loading: true,
  });
  const [tick, setTick] = useState(0);
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;

  useEffect(() => {
    let cancelled = false;
    setState((s) => ({ ...s, loading: true }));
    fetcherRef
      .current()
      .then((data) => !cancelled && setState({ data, error: null, loading: false }))
      .catch((e: unknown) => {
        if (cancelled) return;
        const message = e instanceof ApiError || e instanceof Error ? e.message : String(e);
        setState((s) => ({ data: s.data, error: message, loading: false }));
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, ...deps]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { ...state, reload };
}

/** Calls `fn` every `ms` while the page is visible. */
export function usePoll(fn: () => void, ms: number) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    const id = setInterval(() => {
      if (document.visibilityState === "visible") ref.current();
    }, ms);
    return () => clearInterval(id);
  }, [ms]);
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
