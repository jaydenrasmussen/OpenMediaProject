"use client";

import { createContext, useCallback, useContext, useRef, useState } from "react";

type Kind = "good" | "bad" | "info";
type Notify = (message: string, kind?: Kind) => void;

const ToastContext = createContext<Notify>(() => {});

export const useToast = () => useContext(ToastContext);

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toast, setToast] = useState<{ message: string; kind: Kind } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const notify = useCallback<Notify>((message, kind = "info") => {
    setToast({ message, kind });
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setToast(null), kind === "bad" ? 7000 : 3500);
  }, []);

  return (
    <ToastContext.Provider value={notify}>
      {children}
      {toast && (
        <div className={`toast ${toast.kind === "info" ? "" : toast.kind}`} role="status">
          {toast.message}
        </div>
      )}
    </ToastContext.Provider>
  );
}
