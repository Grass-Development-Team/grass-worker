import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { notificationsApi } from "./notifications.api";
import { NotificationBell } from "./notification-bell";

vi.mock("./notifications.api", () => ({
  notificationsApi: {
    unreadCount: vi.fn(),
    list: vi.fn(),
    markRead: vi.fn(),
    markAllRead: vi.fn(),
  },
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(notificationsApi.unreadCount).mockResolvedValue({ count: 3 });
  vi.mocked(notificationsApi.list).mockResolvedValue({
    notifications: [
      {
        id: "notification-1",
        action: "project.slug_updated",
        title: "Project slug changed",
        project: { id: "project-1", name: "Demo", slug: "demo-site" },
        content: null,
        reason: "Reserved wording",
        target_url: "/projects/project-1",
        read_at: null,
        created_at: "2026-07-31T02:00:00Z",
      },
    ],
    pagination: { page: 1, per_page: 8, total: 1, total_pages: 1 },
  });
  vi.mocked(notificationsApi.markRead).mockResolvedValue({ ok: true });
  vi.mocked(notificationsApi.markAllRead).mockResolvedValue({ updated: 1 });
});

it("opens announcement content in a dialog and marks it as read", async () => {
  vi.mocked(notificationsApi.list).mockResolvedValue({
    notifications: [
      {
        id: "announcement-1",
        action: "site.announcement",
        title: "Maintenance window",
        project: null,
        content: "The service will restart at 10:00 UTC.",
        reason: null,
        target_url: "/notifications",
        read_at: null,
        created_at: "2026-08-03T02:00:00Z",
      },
    ],
    pagination: { page: 1, per_page: 8, total: 1, total_pages: 1 },
  });
  const user = userEvent.setup();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <NotificationBell />
      </MemoryRouter>
    </QueryClientProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "Notifications, 3 unread" }));
  await user.click(await screen.findByRole("button", { name: /Maintenance window/i }));

  expect(await screen.findByRole("dialog")).toHaveTextContent(
    "The service will restart at 10:00 UTC.",
  );
  expect(notificationsApi.markRead.mock.calls[0]?.[0]).toBe("announcement-1");
});

it("opens a compact inbox from the notification bell", async () => {
  const user = userEvent.setup();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <NotificationBell />
      </MemoryRouter>
    </QueryClientProvider>,
  );

  const trigger = await screen.findByRole("button", { name: "Notifications, 3 unread" });
  expect(screen.getByText("3")).toBeInTheDocument();

  await user.click(trigger);

  expect(await screen.findByRole("heading", { name: "Inbox" })).toBeInTheDocument();
  expect(
    screen.getByRole("link", { name: /Project slug changed: Reserved wording/i }),
  ).toHaveAttribute("href", "/projects/project-1");
  expect(screen.queryByText("Platform Admin", { exact: false })).not.toBeInTheDocument();
  expect(screen.getByRole("link", { name: "View all notifications" })).toHaveAttribute(
    "href",
    "/notifications",
  );
});

it("marks every message as read from the inbox footer", async () => {
  const user = userEvent.setup();
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <NotificationBell />
      </MemoryRouter>
    </QueryClientProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "Notifications, 3 unread" }));
  await user.click(await screen.findByRole("button", { name: "Mark all as read" }));

  expect(notificationsApi.markAllRead).toHaveBeenCalledTimes(1);
});
