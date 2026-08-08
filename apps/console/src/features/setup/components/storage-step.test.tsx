import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vite-plus/test";

import { setupApi } from "../setup.api";
import { StorageStep } from "./storage-step";

vi.mock("../setup.api", () => ({
  setupApi: { configureStorage: vi.fn() },
}));

it("keeps the form available while the default local storage error is handled by Toast", async () => {
  vi.mocked(setupApi.configureStorage).mockRejectedValue(new Error("Storage path is unavailable"));

  render(
    <QueryClientProvider client={new QueryClient()}>
      <StorageStep onSuccess={vi.fn()} />
    </QueryClientProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: "Skip for now (use /data)" }));

  await waitFor(() =>
    expect(setupApi.configureStorage).toHaveBeenCalledWith({
      backend: "local",
      local_root: "/data",
    }),
  );
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Skip for now (use /data)" })).toBeEnabled();
});

it("submits a Cloudflare R2 configuration with write-only credentials", async () => {
  const user = userEvent.setup();
  vi.mocked(setupApi.configureStorage).mockResolvedValue({
    configured: true,
    storage: {
      backend: "r2",
      local_root: "/data",
      endpoint: "https://account.r2.cloudflarestorage.com",
      region: "auto",
      bucket: "grass-artifacts",
      prefix: "production",
      force_path_style: false,
      allow_http: false,
      credentials_configured: true,
    },
  });

  render(
    <QueryClientProvider client={new QueryClient()}>
      <StorageStep onSuccess={vi.fn()} />
    </QueryClientProvider>,
  );

  await user.click(screen.getByRole("combobox", { name: "Storage backend" }));
  await user.click(screen.getByRole("option", { name: "Cloudflare R2" }));
  await user.type(screen.getByLabelText("Endpoint"), "https://account.r2.cloudflarestorage.com");
  await user.type(screen.getByLabelText("Bucket"), "  grass-artifacts  ");
  await user.type(screen.getByLabelText("Prefix"), "/production/");
  await user.type(screen.getByLabelText("Access key ID"), "  r2-access  ");
  await user.type(screen.getByLabelText("Secret access key"), "  r2-secret  ");
  await user.click(screen.getByRole("button", { name: "Test and save storage" }));

  await waitFor(() =>
    expect(setupApi.configureStorage).toHaveBeenCalledWith({
      backend: "r2",
      local_root: "/data",
      endpoint: "https://account.r2.cloudflarestorage.com",
      region: "auto",
      bucket: "grass-artifacts",
      prefix: "production",
      force_path_style: false,
      allow_http: false,
      access_key_id: "r2-access",
      secret_access_key: "r2-secret",
    }),
  );
});
