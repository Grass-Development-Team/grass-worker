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
    serve_status: "ready",
    serve_was_ready: true,
    release_status: "pending_review",
    source_branch: "main",
    commit_hash: "abcdef1234567890",
    commit_message: "Ship the landing page",
    preview_host: "apple-banana-landing.cxcs.page",
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
          release_status: "active",
          release_pending: false,
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
  expect(screen.queryByRole("button", { name: /Approve & promote/ })).not.toBeInTheDocument();
});

it("links to the protected preview and disables decisions until Serve is ready", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
    jsonResponse({
      total: 1,
      reviews: [
        {
          ...reviewFixture,
          deployment: { ...reviewFixture.deployment, serve_status: "syncing" },
        },
      ],
    }),
  );

  renderPanel();

  expect(await screen.findByRole("link", { name: "Open preview" })).toHaveAttribute(
    "href",
    "//apple-banana-landing.cxcs.page",
  );
  expect(screen.getByText("Syncing")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Approve$/ })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Reject" })).toBeDisabled();
});

it("allows decisions for a retired review without exposing its old preview", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
    jsonResponse({
      total: 1,
      reviews: [
        {
          ...reviewFixture,
          deployment: {
            ...reviewFixture.deployment,
            serve_status: "retired",
            serve_was_ready: true,
            preview_host: null,
          },
        },
      ],
    }),
  );

  renderPanel();

  expect(await screen.findByText("Retired")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Approve$/ })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Reject" })).toBeEnabled();
  expect(screen.queryByRole("link", { name: "Open preview" })).not.toBeInTheDocument();
});

it("keeps decisions disabled when a retired review never reached Serve ready", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async () =>
    jsonResponse({
      total: 1,
      reviews: [
        {
          ...reviewFixture,
          deployment: {
            ...reviewFixture.deployment,
            serve_status: "retired",
            serve_was_ready: false,
            preview_host: null,
          },
        },
      ],
    }),
  );

  renderPanel();

  expect(await screen.findByText("Retired")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Approve$/ })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Reject" })).toBeDisabled();
});
