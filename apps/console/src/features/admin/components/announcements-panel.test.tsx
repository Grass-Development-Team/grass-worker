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
      getAnnouncement: vi.fn(),
      publishAnnouncement: vi.fn(),
    },
  };
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.getAnnouncement).mockResolvedValue({
    title: "Current notice",
    content: "Current content",
  });
  vi.mocked(adminApi.publishAnnouncement).mockResolvedValue({
    title: "Planned maintenance",
    content: "The service will restart.",
    recipients: 4,
  });
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
  });
  expect(await screen.findByText("Announcement published.")).toBeInTheDocument();
});
