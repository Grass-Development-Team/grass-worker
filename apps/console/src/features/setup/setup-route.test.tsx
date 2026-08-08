import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { SetupRoute } from "./setup-route";
import { setupApi, type SetupStage } from "./setup.api";

vi.mock("./setup.api", () => ({
  setupApi: {
    getSetupState: vi.fn(),
    configureDatabase: vi.fn(),
    createAdmin: vi.fn(),
    configureSite: vi.fn(),
    createNode: vi.fn(),
    configureStorage: vi.fn(),
    finishSetup: vi.fn(),
  },
}));

let stage: SetupStage;

function renderSetup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={["/setup"]}>
        <Routes>
          <Route path="/setup" element={<SetupRoute />} />
          <Route path="/login" element={<div>Login after setup</div>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  stage = "database";
  vi.mocked(setupApi.getSetupState).mockImplementation(async () => ({
    stage,
    is_setup_mode: stage !== "complete",
  }));
  vi.mocked(setupApi.configureDatabase).mockImplementation(async () => {
    stage = "admin";
    return { connected: true, migrations_applied: true, seed_completed: true };
  });
  vi.mocked(setupApi.createAdmin).mockImplementation(async () => {
    stage = "site";
    return {
      user: { id: "user-1", email: "admin@example.com", display_name: "Administrator" },
      team: { id: "team-1", slug: "default", name: "Default" },
    };
  });
  vi.mocked(setupApi.configureSite).mockImplementation(async () => {
    stage = "node";
    return {
      configured: true,
      name: "Grass Worker",
      site_url: "https://console.example.com",
      public_base_url: "https://sites.example.com",
    };
  });
  vi.mocked(setupApi.createNode).mockImplementation(async () => {
    stage = "storage";
    return { node: { id: "node-1", name: "local-node" }, token: "one-time-node-token" };
  });
  vi.mocked(setupApi.configureStorage).mockImplementation(async () => {
    stage = "finish";
    return {
      configured: true,
      storage: {
        backend: "local",
        local_root: "/data",
        endpoint: "",
        region: "us-east-1",
        bucket: "",
        prefix: "",
        force_path_style: false,
        allow_http: false,
        credentials_configured: false,
      },
    };
  });
  vi.mocked(setupApi.finishSetup).mockImplementation(async () => {
    stage = "complete";
    return { setup_finished: true };
  });
});

it("completes the setup workflow and redirects to login", async () => {
  const user = userEvent.setup();
  renderSetup();

  expect(await screen.findByText("Configure Database")).toBeInTheDocument();
  await user.type(screen.getByLabelText("Host"), "db.internal");
  await user.type(screen.getByLabelText("Port"), "5432");
  await user.type(screen.getByLabelText("Username"), "grass");
  await user.type(screen.getByLabelText("Password"), "database-secret");
  await user.type(screen.getByLabelText("Database"), "grass_worker");
  await user.click(screen.getByRole("button", { name: /Connect to Database/ }));

  expect(await screen.findByText("Create Admin Account")).toBeInTheDocument();
  await user.type(screen.getByLabelText("Email"), "admin@example.com");
  await user.type(screen.getByLabelText("Display Name (optional)"), "Administrator");
  await user.type(screen.getByLabelText("Password"), "strong-password");
  await user.click(screen.getByRole("button", { name: /Create Admin/ }));

  expect(await screen.findByText("Configure Site")).toBeInTheDocument();
  await user.clear(screen.getByLabelText("Site Name"));
  await user.type(screen.getByLabelText("Site Name"), "Grass Worker");
  await user.clear(screen.getByLabelText("Site URL"));
  await user.type(screen.getByLabelText("Site URL"), "https://console.example.com");
  await user.clear(screen.getByLabelText("Public Base URL"));
  await user.type(screen.getByLabelText("Public Base URL"), "https://sites.example.com");
  await user.click(screen.getByRole("button", { name: /Save Site Configuration/ }));

  expect(await screen.findByText("Create First Node")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Create Node/ }));
  expect(await screen.findByText("one-time-node-token")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Continue/ }));

  expect(await screen.findByText("Configure Storage")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Skip for now (use /data)" }));

  expect(await screen.findByText("Complete Setup")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Finish Setup/ }));

  expect(await screen.findByText("Login after setup")).toBeInTheDocument();
  expect(setupApi.configureDatabase).toHaveBeenCalledWith(
    "db.internal",
    "5432",
    "grass",
    "database-secret",
    "grass_worker",
  );
  expect(setupApi.createAdmin).toHaveBeenCalledWith(
    "admin@example.com",
    "strong-password",
    "Administrator",
  );
  expect(setupApi.configureSite).toHaveBeenCalledWith(
    "Grass Worker",
    "https://console.example.com",
    "https://sites.example.com",
  );
  expect(setupApi.createNode).toHaveBeenCalledWith("local-node");
  expect(setupApi.configureStorage).toHaveBeenCalledWith({ backend: "local", local_root: "/data" });
  expect(setupApi.finishSetup).toHaveBeenCalledOnce();
});
