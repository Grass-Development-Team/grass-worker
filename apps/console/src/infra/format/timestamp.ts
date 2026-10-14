/** Formats a timestamp with a caller-selected empty value and a safe invalid-date fallback. */
export function formatTimestamp(value: string | null | undefined, fallback: string): string {
  if (!value) return fallback;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "Unknown date" : date.toLocaleString();
}
