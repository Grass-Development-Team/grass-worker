import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi } from "../admin.api";
import { CodePanel } from "./code-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      listCodes: vi.fn(),
      generateCodes: vi.fn(),
      revokeCode: vi.fn(),
    },
  };
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.listCodes).mockResolvedValue({
    scopes: ["registration"],
    codes: [
      {
        id: "code-1",
        code: "ABCDEF...WXYZ",
        scope: "registration",
        status: "used",
        expires_at: "2026-09-05T00:00:00Z",
        used_at: "2026-08-06T01:00:00Z",
        used_by: { id: "user-1", email: "member@example.com", display_name: "Member" },
        revoked_at: null,
        created_at: "2026-08-06T00:00:00Z",
      },
    ],
    pagination: { page: 1, per_page: 50, total: 1, total_pages: 1 },
  });
  vi.mocked(adminApi.generateCodes).mockResolvedValue({
    codes: [
      {
        id: "code-2",
        code: "0123456789abcdefghijklmnopqrstuvwxyzABCD",
        scope: "registration",
        expires_at: "2026-09-05T00:00:00Z",
        created_at: "2026-08-06T00:00:00Z",
      },
    ],
  });
  vi.mocked(adminApi.revokeCode).mockResolvedValue({ code: {} as never });
});

function renderPanel() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <CodePanel />
    </QueryClientProvider>,
  );
}

it("lists masked codes with scope, status, and usage", async () => {
  renderPanel();

  expect(await screen.findByText("ABCDEF...WXYZ")).toBeInTheDocument();
  expect(screen.getByText("registration")).toBeInTheDocument();
  expect(screen.getByText("Used", { selector: "span" })).toBeInTheDocument();
  expect(screen.getByText("Member")).toBeInTheDocument();
  expect(screen.getByText("member@example.com")).toBeInTheDocument();
  expect(screen.queryByText("0123456789abcdefghijklmnopqrstuvwxyzABCD")).not.toBeInTheDocument();
});

it("shows newly generated full codes in a one-time table", async () => {
  const user = userEvent.setup();
  renderPanel();

  await user.click(await screen.findByRole("button", { name: "Generate codes" }));
  const count = screen.getByLabelText("Quantity");
  await user.clear(count);
  await user.type(count, "1");
  await user.click(screen.getByRole("button", { name: "Generate" }));

  await waitFor(() =>
    expect(adminApi.generateCodes).toHaveBeenCalledWith({
      scope: "registration",
      count: 1,
      expires_in_days: 30,
      never_expires: false,
    }),
  );
  expect(await screen.findByText("0123456789abcdefghijklmnopqrstuvwxyzABCD")).toBeInTheDocument();
  expect(screen.getByText("Generated codes")).toBeInTheDocument();
});
