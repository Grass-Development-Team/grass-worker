import { afterEach, expect, it, vi } from "vitest";
import { logStreamUrl } from ".";
afterEach(() => vi.unstubAllEnvs());
it("uses the same relative API prefix for log WebSockets", () => {
  vi.stubEnv("VITE_API_BASE_URL", "/control");
  expect(logStreamUrl("project", "deployment")).toBe(
    "ws://localhost/control/api/v1/projects/project/deployments/deployment/logs/ws",
  );
});
it("uses secure WebSockets for an HTTPS API base URL", () => {
  vi.stubEnv("VITE_API_BASE_URL", "https://api.example.com/");
  expect(logStreamUrl("project", "deployment")).toBe(
    "wss://api.example.com/api/v1/projects/project/deployments/deployment/logs/ws",
  );
});
