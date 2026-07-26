import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { vi } from "vitest";

import { NodeStep } from "./node-step";
import { setupApi } from "../setup.api";

vi.mock("../setup.api", () => ({
  setupApi: { createNode: vi.fn() },
}));

it("keeps the one-time node token visible until the user continues", async () => {
  vi.mocked(setupApi.createNode).mockResolvedValue({
    node: { id: "node-1", name: "local-node" },
    token: "one-time-token",
  });
  const onContinue = vi.fn();

  function Harness() {
    const [token, setToken] = useState<string | null>(null);
    return <NodeStep token={token} onCreated={setToken} onContinue={onContinue} />;
  }

  render(
    <QueryClientProvider client={new QueryClient()}>
      <Harness />
    </QueryClientProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: "Create Node" }));

  expect(await screen.findByText("one-time-token")).toBeInTheDocument();
  expect(onContinue).not.toHaveBeenCalled();

  await userEvent.click(screen.getByRole("button", { name: /Continue/ }));
  expect(onContinue).toHaveBeenCalledOnce();
});

it("copies the one-time node token", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  vi.mocked(setupApi.createNode).mockResolvedValue({
    node: { id: "node-1", name: "local-node" },
    token: "one-time-token",
  });

  function Harness() {
    const [token, setToken] = useState<string | null>(null);
    return <NodeStep token={token} onCreated={setToken} onContinue={vi.fn()} />;
  }
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Harness />
    </QueryClientProvider>,
  );
  await userEvent.click(screen.getByRole("button", { name: "Create Node" }));
  await userEvent.click(await screen.findByRole("button", { name: "Copy node token" }));

  expect(writeText).toHaveBeenCalledWith("one-time-token");
  expect(screen.getByRole("button", { name: "Node token copied" })).toBeInTheDocument();
});
