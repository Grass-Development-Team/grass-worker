import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useTeam } from "@/features/teams/team-context";

import { QuotaRoute } from "./quota-route";

vi.mock("@/features/teams/team-context", () => ({ useTeam: vi.fn() }));

function apiResponse(data: unknown, status = 200, message = "OK"): Response {
  return new Response(JSON.stringify({ code: status, message, data }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function renderQuota() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <QuotaRoute />
    </QueryClientProvider>,
  );
}

describe("QuotaRoute", () => {
  beforeEach(() => {
    vi.mocked(useTeam).mockReturnValue({
      activeTeam: { id: "team-1", slug: "platform", name: "Platform", kind: "team" },
    } as ReturnType<typeof useTeam>);
  });
  afterEach(() => vi.restoreAllMocks());

  it("renders bounded, capped, and unlimited usage from the active plan", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      apiResponse({
        plan: { id: "plan-1", code: "production", name: "Production", source: "explicit" },
        usage: [
          { dimension: "projects", used: 4, limit: 10, period: "none" },
          { dimension: "deployments.monthly", used: 120, limit: 100, period: "monthly" },
          { dimension: "storage_mb", used: 256, limit: null, period: "none" },
        ],
      }),
    );

    renderQuota();

    expect(await screen.findByRole("heading", { name: "Usage & quota" })).toBeInTheDocument();
    expect(screen.getByText("Production")).toBeInTheDocument();
    expect(screen.getByText("production")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Projects usage" })).toHaveAttribute(
      "aria-valuenow",
      "40",
    );
    expect(
      screen.getByRole("progressbar", { name: "Deployments this month usage" }),
    ).toHaveAttribute("aria-valuenow", "100");
    const storageRow = screen.getByText("Artifact storage (MB)").closest("tr");
    expect(storageRow).not.toBeNull();
    expect(within(storageRow as HTMLElement).getByText("Unlimited")).toBeInTheDocument();
  });

  it("shows the API error instead of stale quota data", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      apiResponse(null, 503, "Quota service unavailable"),
    );

    renderQuota();

    expect(await screen.findByRole("alert")).toHaveTextContent("Quota service unavailable");
    expect(screen.queryByRole("heading", { name: "Usage & quota" })).not.toBeInTheDocument();
  });

  it("does not request usage until a team is selected", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    vi.mocked(useTeam).mockReturnValue({ activeTeam: null } as ReturnType<typeof useTeam>);

    renderQuota();

    expect(screen.getByText("Select a team to view quota usage.")).toBeInTheDocument();
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
