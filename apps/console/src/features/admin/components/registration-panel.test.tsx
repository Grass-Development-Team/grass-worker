import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi } from "../admin.api";
import { RegistrationPanel } from "./registration-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listRegistrationEmails: vi.fn(),
      addRegistrationEmail: vi.fn(),
      removeRegistrationEmail: vi.fn(),
    },
  };
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.listRegistrationEmails).mockResolvedValue({
    emails: [
      {
        id: "email-1",
        email: "member@example.com",
        created_at: "2026-08-06T00:00:00Z",
        created_by: {
          id: "admin-1",
          email: "admin@example.com",
          display_name: "Administrator",
        },
      },
    ],
  });
  vi.mocked(adminApi.addRegistrationEmail).mockResolvedValue({
    email: {
      id: "email-2",
      email: "new@example.com",
      created_at: "2026-08-06T01:00:00Z",
      created_by: { id: "admin-1", email: null, display_name: null },
    },
  });
  vi.mocked(adminApi.removeRegistrationEmail).mockResolvedValue({ deleted: true });
});

function renderPanel() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <RegistrationPanel />
    </QueryClientProvider>,
  );
}

it("lists emails authorized for invite-only registration", async () => {
  renderPanel();

  expect(await screen.findByText("member@example.com")).toBeInTheDocument();
  expect(screen.getByText("Administrator")).toBeInTheDocument();
});

it("adds an email to registration authorization", async () => {
  const user = userEvent.setup();
  renderPanel();

  await user.type(await screen.findByLabelText("Email"), "new@example.com");
  await user.click(screen.getByRole("button", { name: "Add email" }));

  await waitFor(() =>
    expect(adminApi.addRegistrationEmail).toHaveBeenCalledWith({ email: "new@example.com" }),
  );
});

it("removes an authorized email after confirmation", async () => {
  const user = userEvent.setup();
  renderPanel();

  await user.click(await screen.findByRole("button", { name: "Remove member@example.com" }));
  await user.click(screen.getByRole("button", { name: "Remove email" }));

  await waitFor(() => expect(adminApi.removeRegistrationEmail).toHaveBeenCalledWith("email-1"));
});
