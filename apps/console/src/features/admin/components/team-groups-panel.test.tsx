import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { TeamGroupsPanel } from "./team-groups-panel";

const response = (data: unknown) => Response.json({ code: 200, message: "OK", data });

afterEach(() => vi.restoreAllMocks());

it("shows and edits the team group review policy override", async () => {
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input: RequestInfo | URL) => {
    if (String(input).endsWith("/team-groups")) {
      return response({
        groups: [
          {
            id: "group-vip",
            code: "vip",
            name: "VIP",
            description: "Priority customers",
            quota_plan_id: null,
            review_policy: { production: "auto", preview: null, domain: "manual" },
            is_default: false,
            team_count: 3,
            created_at: "2026-07-29T00:00:00Z",
          },
        ],
      });
    }
    if (String(input).endsWith("/quota-plans")) return response({ plans: [] });
    throw new Error(`unexpected request: ${input}`);
  });
  const user = userEvent.setup();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <TeamGroupsPanel />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("VIP")).toBeInTheDocument();
  expect(screen.getByRole("columnheader", { name: "Review policy" })).toBeInTheDocument();
  expect(screen.getByText("Production Auto · Preview Inherit · Domain Manual")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Edit" }));
  expect(screen.getByRole("combobox", { name: "Production review" })).toHaveTextContent("Auto");
  expect(screen.getByRole("combobox", { name: "Preview review" })).toHaveTextContent("Inherit");
  expect(screen.getByRole("combobox", { name: "Domain review" })).toHaveTextContent("Manual");
});
