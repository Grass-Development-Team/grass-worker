import { afterEach, describe, expect, it, vi } from "vitest";
import { deploymentsApi } from ".";
import { BuildLogStream } from "./log-stream";
class Socket {
  static instances: Socket[] = [];
  onopen?: () => void;
  onmessage?: (event: { data: string }) => void;
  onclose?: () => void;
  onerror?: () => void;
  send = vi.fn();
  close = vi.fn(() => this.onclose?.());
  constructor(_url: string) {
    Socket.instances.push(this);
  }
}
afterEach(() => {
  vi.restoreAllMocks();
  Socket.instances = [];
});
const line = (seq: number) => ({ seq, stage: "build", line: `Line ${seq}`, timestamp_ms: seq });
describe("build log lifecycle", () => {
  it("backfills gaps, deduplicates frames, and stops reconnecting after disposal", async () => {
    vi.stubGlobal("WebSocket", Socket);
    const catchUp = vi
      .spyOn(deploymentsApi, "buildLog")
      .mockResolvedValueOnce({ lines: [line(1)], last_seq: 1, build_status: "building" })
      .mockResolvedValueOnce({ lines: [line(2), line(3)], last_seq: 3, build_status: "building" });
    const stream = new BuildLogStream("p1", "d1", vi.fn());
    stream.start(true);
    await vi.waitFor(() => expect(Socket.instances).toHaveLength(1));
    const socket = Socket.instances[0];
    socket.onopen?.();
    await vi.waitFor(() => expect(stream.state.value.lines).toHaveLength(3));
    socket.onmessage?.({ data: JSON.stringify({ type: "log", ...line(3) }) });
    expect(stream.state.value.lines.map((line) => line.seq)).toEqual([1, 2, 3]);
    expect(catchUp).toHaveBeenNthCalledWith(2, "p1", "d1", 1, expect.any(AbortSignal));
    stream.stop();
    socket.onmessage?.({ data: JSON.stringify({ type: "log", ...line(4) }) });
    expect(stream.state.value.lines).toHaveLength(3);
    expect(socket.close).toHaveBeenCalledOnce();
  });
  it("uses final catch-up before notifying completion", async () => {
    vi.stubGlobal("WebSocket", Socket);
    vi.spyOn(deploymentsApi, "buildLog")
      .mockResolvedValueOnce({ lines: [line(1)], last_seq: 1, build_status: "building" })
      .mockResolvedValue({ lines: [line(2)], last_seq: 2, build_status: "ready" });
    const done = vi.fn(),
      stream = new BuildLogStream("p1", "d1", done);
    stream.start(true);
    await vi.waitFor(() => expect(Socket.instances).toHaveLength(1));
    Socket.instances[0].onmessage?.({ data: JSON.stringify({ type: "done" }) });
    await vi.waitFor(() => expect(done).toHaveBeenCalledOnce());
    expect(stream.state.value.lines.map((line) => line.seq)).toEqual([1, 2]);
    stream.stop();
  });
});
