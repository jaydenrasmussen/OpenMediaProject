// Thin wrapper over fetch for /api/v1. Same-origin by default; set
// NEXT_PUBLIC_API_BASE when developing against a server on another port.

// A production build is made for the stand-in prefix below; the server swaps
// in the real URL base (a reverse-proxy prefix, or nothing) as it serves files.
const BASE = process.env.NEXT_PUBLIC_API_BASE ?? (process.env.NODE_ENV === "production" ? "/__omp_base__" : "");

/** Where the API lives, for links the browser follows itself (downloads, feeds). */
export const apiUrl = (path: string) => `${BASE}/api/v1${path}`;

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export const UNAUTHORIZED_EVENT = "omp-unauthorized";

export async function api<T = unknown>(
  path: string,
  init: { method?: string; body?: unknown } = {},
): Promise<T> {
  const response = await fetch(`${BASE}/api/v1${path}`, {
    method: init.method ?? "GET",
    credentials: "include",
    headers: init.body === undefined ? undefined : { "content-type": "application/json" },
    body: init.body === undefined ? undefined : JSON.stringify(init.body),
  });
  if (response.status === 204) return undefined as T;

  const text = await response.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    // Not JSON (a proxy error page, say); fall through with the status text.
  }
  if (!response.ok) {
    const message =
      (data as { error?: string } | null)?.error ?? response.statusText ?? "request failed";
    // The login endpoints answer 401 for bad credentials; the app-wide
    // "you were logged out" handling is only for everything else.
    if (response.status === 401 && !path.startsWith("/auth/")) {
      window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
    }
    throw new ApiError(response.status, message);
  }
  return data as T;
}

/** A picture's address through this server's cache, so the browser never talks to the picture host. */
export const imageSrc = (url: string) => `${BASE}/api/v1/image?url=${encodeURIComponent(url)}`;

export const get = <T>(path: string) => api<T>(path);
export const post = <T>(path: string, body?: unknown) => api<T>(path, { method: "POST", body: body ?? {} });
export const put = <T>(path: string, body: unknown) => api<T>(path, { method: "PUT", body });
export const del = (path: string) => api<void>(path, { method: "DELETE" });
