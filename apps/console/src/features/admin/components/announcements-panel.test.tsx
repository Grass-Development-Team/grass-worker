import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi } from "../admin.api";
import { AnnouncementsPanel } from "./announcements-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listAnnouncements: vi.fn(),
      publishAnnouncement: vi.fn(),
      removeAnnouncement: vi.fn(),
    },
  };
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.listAnnouncements).mockResolvedValue({
    announcements: [],
    pagination: { page: 1, per_page: 20, total: 0, total_pages: 0 },
  });
  vi.mocked(adminApi.publishAnnouncement).mockResolvedValue({
    announcement: {
      id: "announcement-1",
      title: "Planned maintenance",
      content: "The service will restart.",
      auto_popup: true,
      published_at: "2026-08-03T02:00:00Z",
    },
    recipients: 4,
  });
  vi.mocked(adminApi.removeAnnouncement).mockResolvedValue({ ok: true });
});

it("publishes a new announcement to active users", async () => {
  const user = userEvent.setup();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AnnouncementsPanel />
    </QueryClientProvider>,
  );

  const title = await screen.findByLabelText("Title");
  const content = screen.getByLabelText("Content");
  await user.clear(title);
  await user.type(title, "Planned maintenance");
  await user.clear(content);
  await user.type(content, "The service will restart.");
  await user.click(screen.getByRole("button", { name: "Publish" }));

  await waitFor(() => expect(adminApi.publishAnnouncement).toHaveBeenCalled());
  expect(adminApi.publishAnnouncement.mock.calls[0]?.[0]).toEqual({
    title: "Planned maintenance",
    content: "The service will restart.",
    auto_popup: false,
  });
  expect(await screen.findByText("Announcement published.")).toBeInTheDocument();
});

it("deletes an announcement from the history", async () => {
  const user = userEvent.setup();
  vi.mocked(adminApi.listAnnouncements).mockResolvedValue({
    announcements: [
      {
        id: "announcement-1",
        title: "Old notice",
        content: "Old content",
        auto_popup: false,
        published_at: "2026-08-02T02:00:00Z",
      },
    ],
    pagination: { page: 1, per_page: 20, total: 1, total_pages: 1 },
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AnnouncementsPanel />
    </QueryClientProvider>,
  );

  await user.click(await screen.findByRole("button", { name: "Delete Old notice" }));
  await user.click(screen.getByRole("button", { name: "Delete announcement" }));

  await waitFor(() =>
    expect(adminApi.removeAnnouncement.mock.calls[0]?.[0]).toBe("announcement-1"),
  );
});
