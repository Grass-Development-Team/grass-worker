import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { ReviewsPanel } from "./reviews-panel";

afterEach(() => vi.restoreAllMocks());

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ code: 200, message: "OK", data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

const reviewFixture = {
  id: "review-1",
  requested_at: new Date().toISOString(),
  deployment: {
    id: "deploy-1",
    environment: "production",
    build_status: "ready",
    release_status: "pending_review",
    source_branch: "main",
    commit_hash: "abcdef1234567890",
    commit_message: "Ship the landing page",
    preview_host: null,
    created_at: new Date().toISOString(),
  },
  project: { id: "project-1", name: "Landing", slug: "landing" },
  team: { id: "team-1", name: "Acme", slug: "acme" },
  triggered_by: { id: "user-1", email: "dev@acme.test", display_name: "Dev" },
};

function renderPanel() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ReviewsPanel />
    </QueryClientProvider>,
  );
}

it("lists pending reviews across teams", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
    jsonResponse({ total: 1, reviews: [reviewFixture] }),
  );

  renderPanel();

  expect(await screen.findByText("Landing")).toBeInTheDocument();
  expect(screen.getByText("Ship the landing page")).toBeInTheDocument();
  expect(screen.getByText("Production")).toBeInTheDocument();
});

it("approves a deployment through the admin decision endpoint", async () => {
  const fetchMock = vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/review/approve")) {
        return jsonResponse({
          deployment_id: "deploy-1",
          release_status: "approved",
          promoted: false,
        });
      }
      return jsonResponse({ total: 1, reviews: [reviewFixture] });
    });

  renderPanel();

  await userEvent.click(await screen.findByRole("button", { name: /^Approve$/ }));

  expect(fetchMock).toHaveBeenCalledWith(
    "/api/v1/admin/deployments/deploy-1/review/approve",
    expect.objectContaining({ method: "POST" }),
  );
});
