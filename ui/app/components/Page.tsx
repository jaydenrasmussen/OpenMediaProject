export function LoadState({
  loading,
  error,
  hasData,
}: {
  loading: boolean;
  error: string | null;
  hasData: boolean;
}) {
  if (error) return <p className="error">{error}</p>;
  if (loading && !hasData) return <p className="muted">Loading…</p>;
  return null;
}
