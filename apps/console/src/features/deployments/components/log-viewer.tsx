import { PauseIcon, PlayIcon, WifiIcon, WifiOffIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

import {
  deploymentsApi,
  isBuildRunning,
  logStreamUrl,
  type BuildLogLine,
  type BuildStatus,
} from "../deployments.api";

interface LogViewerProps {
  projectId: string;
  deploymentId: string;
  buildStatus: BuildStatus;
  /** Called when a `done` frame arrives so the parent can refetch. */
  onDone?: () => void;
}

/**
 * Realtime build log viewer: catches up over HTTP by sequence number, then
 * follows the websocket stream. Reconnects re-run catch-up so no line is
 * lost. Auto-scroll can be paused; scrolling up pauses it implicitly.
 */
export function LogViewer({ projectId, deploymentId, buildStatus, onDone }: LogViewerProps) {
  const [lines, setLines] = useState<BuildLogLine[]>([]);
  const [connected, setConnected] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const lastSeqRef = useRef(0);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const runningRef = useRef(isBuildRunning(buildStatus));
  runningRef.current = isBuildRunning(buildStatus);

  const appendLines = useCallback((incoming: BuildLogLine[]) => {
    const fresh = incoming.filter((line) => line.seq > lastSeqRef.current);
    if (fresh.length === 0) return;
    lastSeqRef.current = Math.max(lastSeqRef.current, ...fresh.map((line) => line.seq));
    setLines((existing) => [...existing, ...fresh]);
  }, []);

  const catchUp = useCallback(async () => {
    try {
      const result = await deploymentsApi.buildLog(projectId, deploymentId, lastSeqRef.current);
      appendLines(result.lines);
    } catch {
      // Catch-up failures are retried on the next reconnect.
    }
  }, [projectId, deploymentId, appendLines]);

  useEffect(() => {
    let disposed = false;
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

    const connect = async () => {
      await catchUp();
      if (disposed || !runningRef.current) return;

      const socket = new WebSocket(logStreamUrl(projectId, deploymentId));
      socketRef.current = socket;

      socket.onopen = () => {
        if (!disposed) setConnected(true);
        socket.send(JSON.stringify({ type: "subscribe", deployment_id: deploymentId }));
      };
      socket.onmessage = (event) => {
        try {
          const frame = JSON.parse(String(event.data)) as {
            type: string;
            seq?: number;
            stage?: string;
            line?: string;
            timestamp_ms?: number;
          };
          if (frame.type === "log" && frame.seq !== undefined) {
            appendLines([
              {
                seq: frame.seq,
                stage: frame.stage ?? "build",
                line: frame.line ?? "",
                timestamp_ms: frame.timestamp_ms ?? Date.now(),
              },
            ]);
          } else if (frame.type === "done") {
            onDone?.();
          }
        } catch {
          // Ignore malformed frames.
        }
      };
      socket.onclose = () => {
        if (disposed) return;
        setConnected(false);
        socketRef.current = null;
        if (runningRef.current) {
          reconnectTimer = setTimeout(connect, 2000);
        }
      };
      socket.onerror = () => socket.close();
    };

    void connect();
    return () => {
      disposed = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      socketRef.current?.close();
      socketRef.current = null;
    };
  }, [projectId, deploymentId, appendLines, catchUp, onDone]);

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [lines, autoScroll]);

  const running = isBuildRunning(buildStatus);

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          {running ? (
            connected ? (
              <Badge variant="success">
                <WifiIcon /> Live
              </Badge>
            ) : (
              <Badge variant="warning">
                <WifiOffIcon /> Reconnecting…
              </Badge>
            )
          ) : (
            <Badge variant="outline">Build finished</Badge>
          )}
          <span>{lines.length} lines</span>
        </div>
        <Button size="sm" variant="ghost" onClick={() => setAutoScroll((value) => !value)}>
          {autoScroll ? (
            <>
              <PauseIcon /> Pause scroll
            </>
          ) : (
            <>
              <PlayIcon /> Follow
            </>
          )}
        </Button>
      </div>
      <div
        ref={containerRef}
        onScroll={(event) => {
          const target = event.currentTarget;
          const atBottom = target.scrollHeight - target.scrollTop - target.clientHeight < 24;
          if (!atBottom && autoScroll) setAutoScroll(false);
        }}
        className="h-96 overflow-y-auto rounded-md border bg-zinc-950 p-3 font-mono text-xs text-zinc-100"
        role="log"
        aria-label="Build log"
      >
        {lines.length === 0 ? (
          <p className="text-zinc-500">
            {running ? "Waiting for build output…" : "No log output was recorded."}
          </p>
        ) : (
          lines.map((line) => (
            <div key={line.seq} className="flex gap-2 whitespace-pre-wrap break-all">
              <span className="shrink-0 select-none text-zinc-500">
                [{line.stage.padEnd(8, " ").slice(0, 8)}]
              </span>
              <span>{line.line}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
