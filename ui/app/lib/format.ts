export function formatSize(bytes: number): string {
  if (!bytes) return "–";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

export function formatAge(hours: number): string {
  if (hours < 1) return `${Math.max(1, Math.round(hours * 60))}m`;
  if (hours < 48) return `${Math.round(hours)}h`;
  const days = hours / 24;
  if (days < 365) return `${Math.round(days)}d`;
  return `${(days / 365).toFixed(1)}y`;
}

export function formatDate(iso: string | null): string {
  if (!iso) return "–";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "–" : d.toLocaleString();
}

export function formatRuntime(minutes: number): string {
  if (!minutes) return "";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h ? `${h}h ${m}m` : `${m}m`;
}

/** TMDB serves "original" size images; a card needs far less. */
export function posterUrl(images: { kind: string; url: string }[], width = 342): string | null {
  const poster = images.find((i) => i.kind === "Poster");
  return poster ? poster.url.replace("/t/p/original/", `/t/p/w${width}/`) : null;
}

export function percent(total: number, left: number): number {
  if (!total) return 0;
  return Math.min(100, Math.max(0, Math.round(((total - left) / total) * 100)));
}
