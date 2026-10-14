import { Store } from "@/infra/state";
import { deploymentsApi, logStreamUrl, type BuildLogLine } from ".";
export interface LogState {
  lines: BuildLogLine[];
  connected: boolean;
  error: string;
}
export class BuildLogStream {
  readonly state = new Store<LogState>({ lines: [], connected: false, error: "" });
  private lines = new Map<number, BuildLogLine>();
  private contiguous = 0;
  private socket: WebSocket | null = null;
  private timer?: ReturnType<typeof setTimeout>;
  private disposed = false;
  private catching: Promise<void> | null = null;
  private running = true;
  private abort = new AbortController();
  constructor(
    private projectId: string,
    private deploymentId: string,
    private onDone: () => void,
  ) {}
  start(running: boolean): void {
    this.running = running;
    void this.connect();
  }
  stop(): void {
    this.disposed = true;
    this.abort.abort();
    clearTimeout(this.timer);
    this.socket?.close();
    this.socket = null;
  }
  setRunning(running: boolean): void {
    this.running = running;
    if (!running) {
      clearTimeout(this.timer);
      this.socket?.close();
    }
  }
  private append(incoming: BuildLogLine[]): void {
    if (this.disposed) return;
    for (const line of incoming) if (!this.lines.has(line.seq)) this.lines.set(line.seq, line);
    while (this.lines.has(this.contiguous + 1)) this.contiguous++;
    this.state.set({
      ...this.state.value,
      lines: [...this.lines.values()].sort((a, b) => a.seq - b.seq),
      error: "",
    });
  }
  private catchUp(): Promise<void> {
    if (this.catching) return this.catching;
    this.catching = deploymentsApi
      .buildLog(this.projectId, this.deploymentId, this.contiguous, this.abort.signal)
      .then((result) => this.append(result.lines))
      .catch(() => {
        if (!this.disposed)
          this.state.set({ ...this.state.value, error: "Log catch-up failed. Retrying…" });
      })
      .finally(() => (this.catching = null));
    return this.catching;
  }
  private async connect(): Promise<void> {
    await this.catchUp();
    if (this.disposed || !this.running) return;
    const socket = new WebSocket(logStreamUrl(this.projectId, this.deploymentId));
    this.socket = socket;
    socket.onopen = () => {
      if (this.disposed) {
        socket.close();
        return;
      }
      this.state.set({ ...this.state.value, connected: true });
      socket.send(JSON.stringify({ type: "subscribe", deployment_id: this.deploymentId }));
      void this.catchUp();
    };
    socket.onmessage = (event) => {
      if (this.disposed) return;
      try {
        const frame = JSON.parse(String(event.data)) as {
          type: string;
          seq?: number;
          line?: string;
          stage?: string;
          timestamp_ms?: number;
        };
        if (frame.type === "log" && Number.isInteger(frame.seq) && typeof frame.line === "string") {
          this.append([
            {
              seq: frame.seq!,
              stage: frame.stage || "build",
              line: frame.line,
              timestamp_ms: frame.timestamp_ms ?? Date.now(),
            },
          ]);
          if (frame.seq! > this.contiguous + 1) void this.catchUp();
        } else if (frame.type === "done") {
          this.running = false;
          void this.catchUp().then(() => {
            if (!this.disposed) this.onDone();
          });
          socket.close();
        }
      } catch {
        /* Ignore malformed stream frames. */
      }
    };
    socket.onclose = () => {
      if (this.disposed) return;
      this.state.set({ ...this.state.value, connected: false });
      if (this.running) this.timer = setTimeout(() => void this.connect(), 2000);
    };
    socket.onerror = () => socket.close();
  }
}
