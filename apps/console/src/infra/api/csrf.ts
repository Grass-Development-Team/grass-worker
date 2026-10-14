let csrfToken: string | null = null;
let revision = 0;
export function getCsrfToken(): string | null {
  return csrfToken;
}
export function getCsrfRevision(): number {
  return revision;
}
export function setCsrfToken(token: string | null): void {
  revision++;
  csrfToken = token;
}
export function restoreCsrfToken(token: string, expectedRevision: number): void {
  if (revision === expectedRevision) setCsrfToken(token);
}
