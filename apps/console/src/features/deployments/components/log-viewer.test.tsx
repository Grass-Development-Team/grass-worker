import { act, render, screen, waitFor, within } from "@testing-library/react";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vite-plus/test";

import { deploymentsApi, type BuildLogLine, type BuildStatus } from "../deployments.api";
import { LogViewer } from "./log-viewer";

vi.mock("../deployments.api", () => ({
  deploymentsApi: { buildLog: vi.fn() },
  isBuildRunning: (status: BuildStatus) =>
    ["pending", "claimed", "queued", "building"].includes(status),
  logStreamUrl: (projectId: string, deploymentId: string) =>
    `ws://console.test/${projectId}/${deploymentId}`,
}));

class ControlledWebSocket {
  static instances: ControlledWebSocket[] = [];

  readonly url: string;
  readonly send = vi.fn();
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;

  constructor(url: string | URL) {
    this.url = String(url);
    ControlledWebSocket.instances.push(this);
  }

  open() {
    this.onopen?.(new Event("open"));
  }

  message(frame: unknown) {
    this.onmessage?.(new MessageEvent("message", { data: JSON.stringify(frame) }));
  }

  serverClose() {
    this.onclose?.(new CloseEvent("close"));
  }

  close() {
    this.serverClose();
  }
}

const originalWebSocket = globalThis.WebSocket;

function line(seq: number, text: string): BuildLogLine {
  return { seq, stage: "build", line: text, timestamp_ms: seq * 1000 };
}

function response(lines: BuildLogLine[]) {
  return {
    lines,
    last_seq: Math.max(0, ...lines.map(({ seq }) => seq)),
    build_status: "building" as const,
  };
}

function renderViewer() {
  return render(
    <LogViewer projectId="project-1" deploymentId="deployment-1" buildStatus="building" />,
  );
}

beforeAll(() => {
  Object.defineProperty(globalThis, "WebSocket", {
    configurable: true,
    value: ControlledWebSocket,
  });
});

afterAll(() => {
  Object.defineProperty(globalThis, "WebSocket", {
    configurable: true,
    value: originalWebSocket,
  });
});

beforeEach(() => {
  ControlledWebSocket.instances = [];
  vi.mocked(deploymentsApi.buildLog).mockReset();
});

afterEach(() => vi.restoreAllMocks());

describe("LogViewer continuity", () => {
  it("deduplicates HTTP and WebSocket lines and renders out-of-order frames by sequence", async () => {
    vi.mocked(deploymentsApi.buildLog).mockResolvedValue(
      response([line(1, "first"), line(3, "third")]),
    );
    renderViewer();

    await waitFor(() => expect(ControlledWebSocket.instances).toHaveLength(1));
    const socket = ControlledWebSocket.instances[0];
    act(() => {
      socket.open();
      socket.message({ type: "log", seq: 3, stage: "build", line: "third duplicate" });
      socket.message({ type: "log", seq: 2, stage: "install", line: "second" });
    });

    expect(socket.url).toBe("ws://console.test/project-1/deployment-1");
    expect(socket.send).toHaveBeenCalledWith(
      JSON.stringify({ type: "subscribe", deployment_id: "deployment-1" }),
    );
    expect(screen.getByText("3 lines")).toBeInTheDocument();
    expect(screen.queryByText("third duplicate")).not.toBeInTheDocument();
    expect(
      within(screen.getByRole("log"))
        .getAllByText(/^(first|second|third)$/)
        .map((node) => node.textContent),
    ).toEqual(["first", "second", "third"]);
  });

  it("reconnects from the contiguous watermark until a gap is filled", async () => {
    vi.mocked(deploymentsApi.buildLog)
      .mockResolvedValueOnce(response([line(1, "first"), line(3, "third")]))
      .mockResolvedValueOnce(response([line(2, "second"), line(3, "third"), line(4, "fourth")]))
      .mockResolvedValueOnce(response([]));
    renderViewer();

    await waitFor(() => expect(ControlledWebSocket.instances).toHaveLength(1));
    let reconnect: TimerHandler | undefined;
    const firstTimer = vi.spyOn(globalThis, "setTimeout").mockImplementation(((
      callback: TimerHandler,
    ) => {
      reconnect = callback;
      return 1;
    }) as typeof setTimeout);
    act(() => ControlledWebSocket.instances[0].serverClose());
    expect(reconnect).toBeTypeOf("function");
    await act(async () => {
      await (reconnect as () => Promise<void>)();
    });
    firstTimer.mockRestore();

    await waitFor(() => expect(ControlledWebSocket.instances).toHaveLength(2));
    expect(deploymentsApi.buildLog).toHaveBeenNthCalledWith(2, "project-1", "deployment-1", 1);
    expect(
      within(screen.getByRole("log"))
        .getAllByText(/^(first|second|third|fourth)$/)
        .map((node) => node.textContent),
    ).toEqual(["first", "second", "third", "fourth"]);

    reconnect = undefined;
    const secondTimer = vi.spyOn(globalThis, "setTimeout").mockImplementation(((
      callback: TimerHandler,
    ) => {
      reconnect = callback;
      return 2;
    }) as typeof setTimeout);
    act(() => ControlledWebSocket.instances[1].serverClose());
    expect(reconnect).toBeTypeOf("function");
    await act(async () => {
      await (reconnect as () => Promise<void>)();
    });
    secondTimer.mockRestore();
    await waitFor(() => expect(ControlledWebSocket.instances).toHaveLength(3));
    expect(deploymentsApi.buildLog).toHaveBeenNthCalledWith(3, "project-1", "deployment-1", 4);
  });
});
