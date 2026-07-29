import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, vi } from "vitest";

import { AuditEventsTable } from "./audit-events-table";
import { auditApi } from "./audit.api";

vi.mock("./audit.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./audit.api")>();
  return {
    ...actual,
    auditApi: {
      ...actual.auditApi,
      listAdmin: vi.fn(),
      listTeam: vi.fn(),
    },
  };
});

const page = {
  events: [
    {
      id: "event-1",
      actor_user_id: "user-1",
      actor_node_id: null,
      actor_type: "user" as const,
      team_id: "team-1",
      visibility: "platform" as const,
      action: "deployment.promoted",
      target_type: "deployment",
      target_id: "deployment-1",
      result: "success" as const,
      reason: null,
      metadata: { project_id: "project-1" },
      request_id: "request-1",
      source_ip: "192.0.2.10",
      user_agent: "Grass Console",
      http_method: "POST",
      request_path: "/api/v1/projects/project-1/deployments/deployment-1/promote",
      status_code: 200,
      duration_ms: 17,
      changes: { before: { status: "draft" }, after: { status: "active" } },
      created_at: "2026-07-29T00:00:00Z",
    },
  ],
  pagination: { page: 1, per_page: 50, total: 1, total_pages: 1 },
};

function renderTable() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AuditEventsTable />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.mocked(auditApi.listAdmin).mockResolvedValue(page);
});

it("renders server-side result filters and pagination controls", async () => {
  renderTable();

  await screen.findByText("deployment.promoted");
  expect(screen.getByLabelText("Filter audit events by result")).toBeInTheDocument();
  expect(screen.getByLabelText("Filter audit events by actor type")).toBeInTheDocument();
  expect(screen.getByLabelText("Filter audit events by target ID")).toBeInTheDocument();
  expect(screen.getByLabelText("Filter audit events from time")).toBeInTheDocument();
  expect(screen.getByLabelText("Filter audit events to time")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Previous page" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Next page" })).toBeDisabled();
  expect(screen.getByText("1 event")).toBeInTheDocument();
});

it("opens complete request details from a table row", async () => {
  const user = userEvent.setup();
  renderTable();

  await user.click(
    await screen.findByRole("button", { name: "View details for deployment.promoted" }),
  );

  expect(screen.getByRole("heading", { name: "Audit event details" })).toBeInTheDocument();
  expect(screen.getByText("request-1")).toBeInTheDocument();
  expect(screen.getByText("192.0.2.10")).toBeInTheDocument();
  expect(screen.getByText(/"status": "active"/)).toBeInTheDocument();
});
