import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vite-plus/test";

import { adminApi, type AdminStorageState } from "../admin.api";
import { StorageSettingsPanel } from "./storage-settings-panel";

vi.mock("../admin.api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../admin.api")>();
  return {
    ...actual,
    adminApi: {
      ...actual.adminApi,
      getStorage: vi.fn(),
      testStorage: vi.fn(),
      createStorageMigration: vi.fn(),
    },
  };
});

const localStorageState = {
  storage: {
    backend: "local",
    local_root: "/var/lib/grass-worker",
    endpoint: "",
    region: "us-east-1",
    bucket: "",
    prefix: "",
    force_path_style: false,
    allow_http: false,
    credentials_configured: false,
  },
  maintenance: false,
  migration: null,
} satisfies AdminStorageState;

function renderPanel() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <StorageSettingsPanel />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(adminApi.getStorage).mockResolvedValue(localStorageState);
  vi.mocked(adminApi.testStorage).mockResolvedValue({ tested: true });
  vi.mocked(adminApi.createStorageMigration).mockResolvedValue({
    migration: {
      id: "migration-1",
      status: "pending",
      source: localStorageState.storage,
      target: {
        ...localStorageState.storage,
        backend: "minio",
        endpoint: "http://minio.internal:9000",
        bucket: "grass-artifacts",
        force_path_style: true,
        allow_http: true,
        credentials_configured: true,
      },
      copied_objects: 0,
      copied_bytes: 0,
      total_objects: null,
      total_bytes: null,
      last_error: null,
      created_at: 1_786_080_000,
      started_at: null,
      finished_at: null,
    },
  });
});

it("tests a target before confirming an object migration", async () => {
  const user = userEvent.setup();
  renderPanel();

  expect(await screen.findByText("Local filesystem")).toBeInTheDocument();
  await user.click(screen.getByRole("combobox", { name: "Target backend" }));
  await user.click(screen.getByRole("option", { name: "MinIO" }));
  await user.type(screen.getByLabelText("Target endpoint"), "http://minio.internal:9000");
  await user.type(screen.getByLabelText("Target bucket"), "grass-artifacts");
  await user.type(screen.getByLabelText("Target access key ID"), "minio-access");
  await user.type(screen.getByLabelText("Target secret access key"), "minio-secret");

  const start = screen.getByRole("button", { name: "Start migration" });
  expect(start).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "Test connection" }));

  const target = {
    backend: "minio",
    local_root: "/var/lib/grass-worker",
    endpoint: "http://minio.internal:9000",
    region: "us-east-1",
    bucket: "grass-artifacts",
    prefix: "",
    force_path_style: true,
    allow_http: true,
    access_key_id: "minio-access",
    secret_access_key: "minio-secret",
  };
  await waitFor(() => expect(adminApi.testStorage).toHaveBeenCalledWith(target));
  expect(screen.getByText("Connection verified")).toBeInTheDocument();
  expect(start).toBeEnabled();

  await user.click(start);
  const dialog = screen.getByRole("alertdialog");
  await user.click(within(dialog).getByRole("button", { name: "Start migration" }));
  await waitFor(() => expect(adminApi.createStorageMigration).toHaveBeenCalledWith(target));
});

it("shows maintenance progress while object writes are paused", async () => {
  vi.mocked(adminApi.getStorage).mockResolvedValue({
    ...localStorageState,
    maintenance: true,
    migration: {
      id: "migration-1",
      status: "running",
      source: localStorageState.storage,
      target: {
        ...localStorageState.storage,
        backend: "r2",
        endpoint: "https://account.r2.cloudflarestorage.com",
        region: "auto",
        bucket: "grass-artifacts",
        credentials_configured: true,
      },
      copied_objects: 5,
      copied_bytes: 500,
      total_objects: 10,
      total_bytes: 1_000,
      last_error: null,
      created_at: 1_786_080_000,
      started_at: 1_786_080_001,
      finished_at: null,
    },
  });

  renderPanel();

  expect(await screen.findByText("Object writes are paused")).toBeInTheDocument();
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "50");
  expect(screen.getByText("5 of 10 objects")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
});

it("shows the full redacted active remote configuration", async () => {
  vi.mocked(adminApi.getStorage).mockResolvedValue({
    ...localStorageState,
    storage: {
      backend: "minio",
      local_root: "/srv/grass-node",
      endpoint: "http://minio.internal:9000",
      region: "us-east-1",
      bucket: "grass-artifacts",
      prefix: "production/assets",
      force_path_style: true,
      allow_http: true,
      credentials_configured: true,
    },
  });

  renderPanel();

  expect((await screen.findAllByText("MinIO")).length).toBeGreaterThan(0);
  expect(screen.getByText("grass-artifacts")).toBeInTheDocument();
  expect(screen.getByText("production/assets")).toBeInTheDocument();
  expect(screen.getByText("http://minio.internal:9000")).toBeInTheDocument();
  expect(screen.getByText("/srv/grass-node")).toBeInTheDocument();
  expect(screen.getByText("Path-style requests")).toBeInTheDocument();
  expect(screen.getByText("HTTP allowed")).toBeInTheDocument();
  expect(screen.getByText("Credentials configured")).toBeInTheDocument();
});

it("shows an empty completed migration at full progress", async () => {
  vi.mocked(adminApi.getStorage).mockResolvedValue({
    ...localStorageState,
    migration: {
      id: "migration-empty",
      status: "succeeded",
      source: localStorageState.storage,
      target: { ...localStorageState.storage, local_root: "/srv/grass-worker" },
      copied_objects: 0,
      copied_bytes: 0,
      total_objects: 0,
      total_bytes: 0,
      last_error: null,
      created_at: 1_786_080_000,
      started_at: 1_786_080_001,
      finished_at: 1_786_080_002,
    },
  });

  renderPanel();

  expect(await screen.findByText("Migration completed")).toBeInTheDocument();
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "100");
});
