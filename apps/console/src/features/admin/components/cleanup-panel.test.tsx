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
    deletable: 2,
    skipped: 1,
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
    expect.objectContaining({ action: "auth." }),
  );

  await user.click(screen.getByRole("button", { name: "Delete Audit Events" }));
  expect(screen.getByRole("heading", { name: "Delete audit events?" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Delete", exact: true }));

  await waitFor(() => expect(cleanupApi.deleteAudit).toHaveBeenCalledWith({ action: "auth." }));
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
