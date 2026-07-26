import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vite-plus/test";

import { AdminRoute } from "./admin-route";

afterEach(() => vi.restoreAllMocks());

it("loads system status through the protected administration API", async () => {
  const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
    new Response(
      JSON.stringify({
        code: 200,
        message: "OK",
        data: {
          service: "Grass Worker Control API",
          mode: "ready",
          version: "9.9.9",
        },
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    ),
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={queryClient}>
      <AdminRoute />
    </QueryClientProvider>,
  );

  expect(await screen.findByText("Ready mode | Version 9.9.9")).toBeInTheDocument();
  expect(fetchMock).toHaveBeenCalledWith(
    "/api/v1/admin/status",
    expect.objectContaining({ credentials: "include" }),
  );
});
