import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useAuth } from "@/features/auth/auth-context";
import { useTeam } from "@/features/teams/team-context";
import { DashboardRoute } from "./dashboard-route";

vi.mock("@/features/auth/auth-context", () => ({ useAuth: vi.fn() }));
vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));

let announcementResponse: unknown;

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function renderDashboard(
  platformRole: "admin" | "user",
  announcementData: unknown = { projects: [] },
) {
  announcementResponse = announcementData;
  vi.mocked(useAuth).mockReturnValue({
    user: {
      id: "user-1",
      email: "user@example.com",
      display_name: "User",
      platform_role: platformRole,
    },
  } as ReturnType<typeof useAuth>);
  vi.mocked(useTeam).mockReturnValue({
    activeTeam: { id: "team-1", slug: "team", name: "Team", kind: "team" },
    activeRole: "owner",
  } as ReturnType<typeof useTeam>);

  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <DashboardRoute />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("Dashboard administration shortcut", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    announcementResponse = { projects: [] };
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      return String(input).includes("/announcements")
        ? jsonResponse(announcementResponse)
        : jsonResponse({ projects: [] });
    });
  });
  afterEach(() => vi.restoreAllMocks());

  it("hides Administration from a regular platform user", () => {
    renderDashboard("user");
    expect(screen.queryByRole("link", { name: "Administration" })).not.toBeInTheDocument();
  });

  it("shows Administration to a platform administrator", () => {
    renderDashboard("admin");
    expect(screen.getByRole("link", { name: "Administration" })).toBeInTheDocument();
  });

  it("shows announcement history on the overview page", async () => {
    renderDashboard("user", {
      announcements: [
        {
          id: "announcement-1",
          title: "Maintenance window",
          content: "The service will restart shortly.",
          auto_popup: false,
          published_at: "2026-08-03T02:00:00Z",
        },
      ],
      pagination: { page: 1, per_page: 5, total: 1, total_pages: 1 },
    });

    expect(await screen.findByRole("button", { name: /Maintenance window/i })).toBeInTheDocument();
  });

  it("hides the announcement section when there is no announcement history", async () => {
    renderDashboard("user", {
      announcements: [],
      pagination: { page: 1, per_page: 5, total: 0, total_pages: 0 },
    });

    await screen.findByText("Workspace controls");
    expect(screen.queryByRole("heading", { name: "Announcements" })).not.toBeInTheDocument();
  });

  it("keeps announcements below the workspace controls", async () => {
    renderDashboard("user", {
      announcements: [
        {
          id: "announcement-1",
          title: "Maintenance window",
          content: "The service will restart shortly.",
          auto_popup: false,
          published_at: "2026-08-03T02:00:00Z",
        },
      ],
      pagination: { page: 1, per_page: 5, total: 1, total_pages: 1 },
    });

    const controls = await screen.findByText("Workspace controls");
    const announcements = await screen.findByRole("heading", { name: "Announcements" });
    expect(
      controls.compareDocumentPosition(announcements) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });
});
