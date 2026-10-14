import { API_UNAUTHORIZED_EVENT } from "@/infra/api";
import { setCsrfToken } from "@/infra/api/csrf";
import { queryClient } from "@/infra/query";
import { Store } from "@/infra/state";
import { createContext } from "@lit/context";
import { authApi, isAuthResponse, type AuthUser, type LoginResponse } from ".";
export interface SessionState {
  user: AuthUser | null;
  loading: boolean;
}
export const session = new Store<SessionState>({ user: null, loading: true });
export const sessionContext = createContext<Store<SessionState>>("grass-worker.session");
let revision = 0;
export function commitSession(user: AuthUser): void {
  revision++;
  session.set({ user, loading: false });
}
export function clearSession(): void {
  revision++;
  setCsrfToken(null);
  queryClient.removeQueries({
    predicate: (query) =>
      !["health", "auth-configuration", "announcements", "regions"].includes(
        String(query.queryKey[0]),
      ),
  });
  session.set({ user: null, loading: false });
}
export async function restoreSession(): Promise<void> {
  const version = revision;
  try {
    const data = await authApi.restore();
    if (revision === version) session.set({ user: data.user, loading: false });
  } catch {
    if (revision === version) clearSession();
  }
}
export function acceptLogin(result: LoginResponse): boolean {
  if (!isAuthResponse(result)) return false;
  commitSession(result.user);
  return true;
}
export function listenSession(): () => void {
  window.addEventListener(API_UNAUTHORIZED_EVENT, clearSession);
  return () => window.removeEventListener(API_UNAUTHORIZED_EVENT, clearSession);
}
