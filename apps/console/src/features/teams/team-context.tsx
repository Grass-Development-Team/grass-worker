import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

import { selectActiveTeam } from "./team-selection";
import { teamsApi, type Team, type TeamRole } from "./teams.api";

export const ACTIVE_TEAM_STORAGE_KEY = "grass-worker.active-team-id";
export const teamKeys = {
  all: ["teams"] as const,
  detail: (teamId: string) => ["teams", teamId] as const,
  members: (teamId: string) => ["teams", teamId, "members"] as const,
};

interface TeamState {
  teams: Team[];
  activeTeam: Team | null;
  activeRole: TeamRole | null;
  isLoading: boolean;
  error: Error | null;
  selectTeam: (teamId: string) => void;
  createTeam: (input: { name: string; slug: string }) => Promise<Team>;
  refreshTeams: () => Promise<void>;
}

const TeamContext = createContext<TeamState | null>(null);

export function TeamProvider({ children }: { children: React.ReactNode }) {
  const queryClient = useQueryClient();
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(() =>
    localStorage.getItem(ACTIVE_TEAM_STORAGE_KEY),
  );
  const [createdTeams, setCreatedTeams] = useState<Team[]>([]);
  const query = useQuery({
    queryKey: teamKeys.all,
    queryFn: teamsApi.list,
  });

  const teams = useMemo(() => {
    const fetched = query.data?.teams ?? [];
    return [
      ...createdTeams,
      ...fetched.filter((team) => !createdTeams.some(({ id }) => id === team.id)),
    ];
  }, [createdTeams, query.data?.teams]);
  const activeTeam = useMemo(
    () => selectActiveTeam(teams, selectedTeamId),
    [selectedTeamId, teams],
  );
  const detailQuery = useQuery({
    queryKey: activeTeam ? teamKeys.detail(activeTeam.id) : ["teams", "none"],
    queryFn: () => teamsApi.get(activeTeam!.id),
    enabled: Boolean(activeTeam),
  });
  const activeRole = detailQuery.data?.team.role ?? null;

  useEffect(() => {
    if (!query.isSuccess) return;
    if (activeTeam) {
      if (selectedTeamId !== activeTeam.id) setSelectedTeamId(activeTeam.id);
      localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, activeTeam.id);
    } else {
      if (selectedTeamId !== null) setSelectedTeamId(null);
      localStorage.removeItem(ACTIVE_TEAM_STORAGE_KEY);
    }
  }, [activeTeam, query.isSuccess, selectedTeamId]);

  const selectTeam = useCallback((teamId: string) => {
    setSelectedTeamId(teamId);
    localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, teamId);
  }, []);

  const createTeam = useCallback(
    async (input: { name: string; slug: string }) => {
      const { team } = await teamsApi.create(input);
      setCreatedTeams((current) => [team, ...current.filter(({ id }) => id !== team.id)]);
      setSelectedTeamId(team.id);
      localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, team.id);
      await queryClient.invalidateQueries({ queryKey: teamKeys.all });
      return team;
    },
    [queryClient],
  );

  const refreshTeams = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: teamKeys.all });
  }, [queryClient]);

  const value = useMemo<TeamState>(
    () => ({
      teams,
      activeTeam,
      activeRole,
      isLoading: query.isLoading || (Boolean(activeTeam) && detailQuery.isLoading),
      error: query.error ?? detailQuery.error,
      selectTeam,
      createTeam,
      refreshTeams,
    }),
    [
      activeRole,
      activeTeam,
      createTeam,
      detailQuery.isLoading,
      detailQuery.error,
      query.error,
      query.isLoading,
      refreshTeams,
      selectTeam,
      teams,
    ],
  );

  return <TeamContext.Provider value={value}>{children}</TeamContext.Provider>;
}

export function useTeam(): TeamState {
  const context = useContext(TeamContext);
  if (!context) throw new Error("useTeam must be used within TeamProvider");
  return context;
}
