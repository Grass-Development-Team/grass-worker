import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vite-plus/test";

import { setupApi } from "../setup.api";
import { StorageStep } from "./storage-step";

vi.mock("../setup.api", () => ({
  setupApi: { configureStorage: vi.fn() },
}));

it("shows an error when using the default storage path fails", async () => {
  vi.mocked(setupApi.configureStorage).mockRejectedValue(new Error("Storage path is unavailable"));

  render(
    <QueryClientProvider client={new QueryClient()}>
      <StorageStep onSuccess={vi.fn()} />
    </QueryClientProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: "Skip for now (use /data)" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("Storage path is unavailable");
});
