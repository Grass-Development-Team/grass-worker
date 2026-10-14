import { queryClient } from "@/infra/query";
import { Store } from "@/infra/state";
import { teamsApi, type Team, type TeamRole } from ".";
import { selectActiveTeam } from "./selection";
export const ACTIVE_TEAM_STORAGE_KEY = "grass-worker.active-team-id";
interface TeamState {
  teams: Team[];
  active: Team | null;
  role: TeamRole | null;
  loading: boolean;
  error: Error | null;
}
export const teams = new Store<TeamState>({
  teams: [],
  active: null,
  role: null,
  loading: false,
  error: null,
});
let revision = 0;
function selectedId(): string | null {
  try {
    return localStorage.getItem(ACTIVE_TEAM_STORAGE_KEY);
  } catch {
    return null;
  }
}
export function clearTeams(): void {
  revision++;
  teams.set({ teams: [], active: null, role: null, loading: false, error: null });
}
export async function refreshTeams(preferred?: string): Promise<void> {
  const version = ++revision;
  teams.set({ ...teams.value, loading: true, role: null, error: null });
  try {
    const { teams: list } = await teamsApi.list();
    if (version !== revision) return;
    const active = selectActiveTeam(list, preferred ?? selectedId());
    const role = active ? ((await teamsApi.get(active.id)).team.role ?? null) : null;
    if (version !== revision) return;
    try {
      if (active) localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, active.id);
      else localStorage.removeItem(ACTIVE_TEAM_STORAGE_KEY);
    } catch {
      /* Storage may be unavailable. */
    }
    teams.set({ teams: list, active, role, loading: false, error: null });
  } catch (cause) {
    if (version === revision)
      teams.set({
        ...teams.value,
        loading: false,
        error: cause instanceof Error ? cause : new Error("Unable to load teams."),
      });
  }
}
export async function selectTeam(id: string): Promise<void> {
  await queryClient.cancelQueries();
  queryClient.removeQueries({
    predicate: (query) =>
      query.queryKey.includes("team") ||
      query.queryKey.includes("project") ||
      query.queryKey.includes("deployment"),
  });
  await refreshTeams(id);
}
