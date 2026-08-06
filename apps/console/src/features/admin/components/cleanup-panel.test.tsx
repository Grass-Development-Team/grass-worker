import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { cleanupApi } from "../cleanup.api";
import { CleanupPanel } from "./cleanup-panel";

vi.mock("../cleanup.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../cleanup.api")>();
  return {
    ...actual,
    cleanupApi: {
      ...actual.cleanupApi,
      previewAudit: vi.fn(),
      deleteAudit: vi.fn(),
      previewBuildLogs: vi.fn(),
      deleteBuildLogs: vi.fn(),
    },
  };
});

function renderPanel() {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <CleanupPanel />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.mocked(cleanupApi.previewAudit).mockResolvedValue({
    matched: 3,
    deletable: 3,
    skipped: 0,
    snapshot_before: 1_775_000_000_000,
    events: [
      {
        id: "event-1",
        actor_user_id: "user-1",
        actor_node_id: null,
        actor_type: "user",
        team_id: "team-1",
        visibility: "platform",
        action: "auth.login",
        target_type: "authentication",
        target_id: null,
        result: "success",
        reason: null,
        metadata: {},
        request_id: "request-1",
        source_ip: "192.0.2.10",
        user_agent: "Grass Console",
        http_method: "POST",
        request_path: "/api/v1/auth/login",
        status_code: 200,
        duration_ms: 12,
        changes: {},
        created_at: "2026-04-01T00:00:00Z",
      },
    ],
    pagination: { page: 1, per_page: 25, total: 3, total_pages: 1 },
  });
  vi.mocked(cleanupApi.deleteAudit).mockResolvedValue({ deleted: 2, skipped: 0 });
  vi.mocked(cleanupApi.previewBuildLogs).mockResolvedValue({
    matched: 4,
    deletable: 3,
    skipped: 1,
  });
  vi.mocked(cleanupApi.deleteBuildLogs).mockResolvedValue({
    deleted: 3,
    skipped: 1,
    failed: 0,
  });
});

it("previews and deletes the filtered audit events", async () => {
  const user = userEvent.setup();
  renderPanel();

  await user.type(screen.getByLabelText("Cleanup audit events by action"), "auth.");
  await user.click(screen.getByRole("button", { name: "Preview Audit Events" }));

  expect(await screen.findByText("3 matched")).toBeInTheDocument();
  expect(cleanupApi.previewAudit).toHaveBeenCalledWith(
    expect.objectContaining({ action: "auth.", page: 1, per_page: 25 }),
  );
  expect(screen.getByText("auth.login")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "View details for auth.login" })).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Delete Audit Events" }));
  expect(screen.getByRole("heading", { name: "Delete audit events?" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Delete", exact: true }));

  await waitFor(() =>
    expect(cleanupApi.deleteAudit).toHaveBeenCalledWith(
      expect.objectContaining({
        action: "auth.",
        snapshot_before: 1_775_000_000_000,
      }),
    ),
  );
  expect(await screen.findByText("Deleted 2 records.")).toBeInTheDocument();
});

it("keeps build-log cleanup as a separate protected action", async () => {
  const user = userEvent.setup();
  renderPanel();

  await user.click(screen.getByRole("button", { name: "Preview Build Logs" }));
  expect(await screen.findByText("4 matched")).toBeInTheDocument();
  expect(screen.getByText("1 protected")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Delete Build Logs" }));
  await user.click(screen.getByRole("button", { name: "Delete", exact: true }));

  await waitFor(() => expect(cleanupApi.deleteBuildLogs).toHaveBeenCalledWith({}));
  expect(await screen.findByText(/Deleted 3 records/)).toBeInTheDocument();
});
