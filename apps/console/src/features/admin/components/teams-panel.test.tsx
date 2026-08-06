import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi, type AdminTeam } from "../admin.api";
import { TeamsPanel } from "./teams-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listTeams: vi.fn(),
      batchTeams: vi.fn(),
    },
  };
});

const teams: AdminTeam[] = [
  {
    id: "team-1",
    slug: "team-one",
    name: "Team one",
    kind: "team",
    group: { id: "group-1", code: "standard", name: "Standard" },
    explicit_quota_plan_id: null,
    member_count: 2,
    created_at: "2026-08-04T00:00:00Z",
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.listTeams).mockResolvedValue({ teams });
  vi.mocked(adminApi.batchTeams).mockResolvedValue({
    results: [{ id: "team-1", success: true }],
  });
});

it("filters teams and deletes selected standard teams", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={client}>
      <TeamsPanel />
    </QueryClientProvider>,
  );

  await screen.findByText("Team one");
  await user.click(screen.getByRole("combobox", { name: "Team kind" }));
  await user.click(screen.getByRole("option", { name: "Team" }));
  await waitFor(() => expect(adminApi.listTeams).toHaveBeenLastCalledWith({ kind: "team" }));

  await user.click(screen.getByRole("checkbox", { name: "Select Team one" }));
  await user.click(screen.getByRole("button", { name: "Bulk actions" }));
  await user.click(screen.getByRole("menuitem", { name: "Delete selected" }));
  await waitFor(() =>
    expect(adminApi.batchTeams).toHaveBeenCalledWith({
      action: "delete",
      ids: ["team-1"],
    }),
  );
});
