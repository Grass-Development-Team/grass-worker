import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { notificationsApi } from "./notifications.api";
import { NotificationBell } from "./notification-bell";

vi.mock("./notifications.api", () => ({
  notificationsApi: {
    unreadCount: vi.fn(),
  },
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(notificationsApi.unreadCount).mockResolvedValue({ count: 3 });
});

it("links to notifications and shows the unread count", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <NotificationBell />
      </MemoryRouter>
    </QueryClientProvider>,
  );

  expect(await screen.findByRole("link", { name: "Notifications, 3 unread" })).toHaveAttribute(
    "href",
    "/notifications",
  );
  expect(screen.getByText("3")).toBeInTheDocument();
});
