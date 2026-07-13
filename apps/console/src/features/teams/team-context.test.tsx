import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { Team } from "./teams.api";
import { teamsApi } from "./teams.api";
import { ACTIVE_TEAM_STORAGE_KEY, TeamProvider, useTeam } from "./team-context";

vi.mock("./teams.api", async (importOriginal) => {
  const original = await importOriginal<typeof import("./teams.api")>();
  return {
    ...original,
    teamsApi: {
      ...original.teamsApi,
      list: vi.fn(),
      get: vi.fn(),
      create: vi.fn(),
    },
  };
});

const shared: Team = {
  id: "shared",
  slug: "shared",
  name: "Shared",
  kind: "team",
  owner_user_id: "user-1",
  group_id: null,
};
const personal: Team = { ...shared, id: "personal", name: "Personal", kind: "personal" };

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <TeamProvider>{children}</TeamProvider>
    </QueryClientProvider>
  );
}

describe("TeamProvider", () => {
  beforeEach(() => {
    vi.mocked(teamsApi.get).mockImplementation(async (teamId) => ({
      team: { ...(teamId === "personal" ? personal : shared), role: "owner" },
    }));
  });

  afterEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  it("restores and persists the selected team", async () => {
    localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, "shared");
    vi.mocked(teamsApi.list).mockResolvedValue({ teams: [personal, shared] });

    const { result } = renderHook(() => useTeam(), { wrapper });

    await waitFor(() => expect(result.current.activeTeam?.id).toBe("shared"));
    await waitFor(() => expect(result.current.activeRole).toBe("owner"));
    act(() => result.current.selectTeam("personal"));

    expect(result.current.activeTeam?.id).toBe("personal");
    expect(localStorage.getItem(ACTIVE_TEAM_STORAGE_KEY)).toBe("personal");
  });

  it("repairs an invalid persisted team with the personal team", async () => {
    localStorage.setItem(ACTIVE_TEAM_STORAGE_KEY, "removed");
    vi.mocked(teamsApi.list).mockResolvedValue({ teams: [shared, personal] });

    const { result } = renderHook(() => useTeam(), { wrapper });

    await waitFor(() => expect(result.current.activeTeam?.id).toBe("personal"));
    expect(localStorage.getItem(ACTIVE_TEAM_STORAGE_KEY)).toBe("personal");
  });

  it("selects a newly created team", async () => {
    vi.mocked(teamsApi.list).mockResolvedValue({ teams: [personal] });
    vi.mocked(teamsApi.create).mockResolvedValue({ team: shared });

    const { result } = renderHook(() => useTeam(), { wrapper });
    await waitFor(() => expect(result.current.activeTeam?.id).toBe("personal"));

    await act(() => result.current.createTeam({ name: "Shared", slug: "shared" }));

    expect(result.current.activeTeam?.id).toBe("shared");
    expect(localStorage.getItem(ACTIVE_TEAM_STORAGE_KEY)).toBe("shared");
  });
});
