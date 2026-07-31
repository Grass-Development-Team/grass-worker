import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { notificationsApi } from "./notifications.api";
import { NotificationsRoute } from "./notifications-route";

vi.mock("./notifications.api", () => ({
  notificationsApi: {
    list: vi.fn(),
    unreadCount: vi.fn(),
    markRead: vi.fn(),
    markAllRead: vi.fn(),
  },
}));

const page = {
  notifications: [
    {
      id: "notification-1",
      action: "project.slug_updated",
      title: "Project slug changed",
      project: { id: "project-1", name: "Demo", slug: "demo-site" },
      actor: { id: "admin-1", label: "Platform Admin" },
      reason: "Reserved wording",
      target_url: "/projects/project-1",
      read_at: null,
      created_at: "2026-07-31T02:00:00Z",
    },
    {
      id: "notification-2",
      action: "domain.approved",
      title: "Domain approved",
      project: { id: "project-1", name: "Demo", slug: "demo-site" },
      actor: { id: "admin-1", label: "Platform Admin" },
      reason: null,
      target_url: "/projects/project-1/domains",
      read_at: "2026-07-31T02:30:00Z",
      created_at: "2026-07-31T01:00:00Z",
    },
  ],
  pagination: { page: 1, per_page: 25, total: 2, total_pages: 1 },
};

function renderRoute() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <NotificationsRoute />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("Notifications route", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(notificationsApi.list).mockResolvedValue(page);
    vi.mocked(notificationsApi.unreadCount).mockResolvedValue({ count: 1 });
    vi.mocked(notificationsApi.markAllRead).mockResolvedValue({ updated: 1 });
    vi.mocked(notificationsApi.markRead).mockResolvedValue({ ok: true });
  });

  it("shows governance context, optional reason, unread state and target links", async () => {
    renderRoute();

    expect(await screen.findByText("Project slug changed")).toBeInTheDocument();
    expect(screen.getAllByText("Demo")).toHaveLength(2);
    expect(screen.getAllByText("Platform Admin", { exact: false, selector: "p" })).toHaveLength(2);
    expect(screen.getByText("Reserved wording")).toBeInTheDocument();
    expect(screen.getByText("Unread")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Project slug changed" })).toHaveAttribute(
      "href",
      "/projects/project-1",
    );
  });

  it("marks every unread notification as read", async () => {
    const user = userEvent.setup();
    renderRoute();

    await user.click(await screen.findByRole("button", { name: "Mark all as read" }));

    await waitFor(() => expect(notificationsApi.markAllRead).toHaveBeenCalledTimes(1));
  });

  it("keeps mark-all available when unread messages exist on another page", async () => {
    vi.mocked(notificationsApi.list).mockResolvedValue({
      notifications: page.notifications.map((item) => ({
        ...item,
        read_at: "2026-07-31T03:00:00Z",
      })),
      pagination: { page: 1, per_page: 25, total: 26, total_pages: 2 },
    });

    renderRoute();

    const button = await screen.findByRole("button", { name: "Mark all as read" });
    await waitFor(() => expect(button).toBeEnabled());
  });
});
