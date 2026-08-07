import { useMutation } from "@tanstack/react-query";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AuditActorType, AuditFilters, AuditResult } from "@/features/audit/audit.api";
import { AuditEventResults } from "@/features/audit/audit-events-table";

import {
  cleanupApi,
  type AuditCleanupPreview,
  type BuildLogCleanupFilters,
  type CleanupPreview,
  type CleanupResult,
} from "../cleanup.api";

type AuditForm = {
  action: string;
  actorUserId: string;
  actorType: AuditActorType | "all";
  targetType: string;
  targetId: string;
  teamId: string;
  result: AuditResult | "all";
  from: string;
  to: string;
};

type BuildLogForm = {
  deploymentId: string;
  projectId: string;
  teamId: string;
  triggeredByUserId: string;
  from: string;
  to: string;
};

const initialAudit: AuditForm = {
  action: "",
  actorUserId: "",
  actorType: "all",
  targetType: "",
  targetId: "",
  teamId: "",
  result: "all",
  from: "",
  to: "",
};

const initialBuildLogs: BuildLogForm = {
  deploymentId: "",
  projectId: "",
  teamId: "",
  triggeredByUserId: "",
  from: "",
  to: "",
};

function timestamp(value: string) {
  return value ? new Date(value).getTime() : undefined;
}

function auditFilters(form: AuditForm): AuditFilters {
  return {
    action: form.action.trim() || undefined,
    actor_user_id: form.actorUserId.trim() || undefined,
    actor_type: form.actorType === "all" ? undefined : form.actorType,
    target_type: form.targetType.trim() || undefined,
    target_id: form.targetId.trim() || undefined,
    team_id: form.teamId.trim() || undefined,
    result: form.result === "all" ? undefined : form.result,
    from: timestamp(form.from),
    to: timestamp(form.to),
  };
}

function buildLogFilters(form: BuildLogForm): BuildLogCleanupFilters {
  return {
    deployment_id: form.deploymentId.trim() || undefined,
    project_id: form.projectId.trim() || undefined,
    team_id: form.teamId.trim() || undefined,
    triggered_by_user_id: form.triggeredByUserId.trim() || undefined,
    from: timestamp(form.from),
    to: timestamp(form.to),
  };
}

function PreviewSummary({ preview }: { preview: CleanupPreview | null }) {
  if (!preview) {
    return <p className="text-sm text-muted-foreground">Run a preview to see matching records.</p>;
  }
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm" role="status">
      <Badge variant="outline">{preview.matched} matched</Badge>
      <Badge variant="success">{preview.deletable} deletable</Badge>
      {preview.skipped > 0 && <Badge variant="warning">{preview.skipped} protected</Badge>}
    </div>
  );
}

function ResultMessage({ result }: { result: CleanupResult | null }) {
  if (!result) return null;
  return (
    <p className="text-sm text-muted-foreground" role="status">
      Deleted {result.deleted} record{result.deleted === 1 ? "" : "s"}.
      {result.skipped > 0
        ? ` Skipped ${result.skipped} protected record${result.skipped === 1 ? "" : "s"}.`
        : ""}
      {result.failed && result.failed > 0
        ? ` Failed to remove ${result.failed} file${result.failed === 1 ? "" : "s"}.`
        : ""}
    </p>
  );
}

export function CleanupPanel() {
  const [audit, setAudit] = useState(initialAudit);
  const [buildLogs, setBuildLogs] = useState(initialBuildLogs);
  const [auditPreview, setAuditPreview] = useState<AuditCleanupPreview | null>(null);
  const [buildLogPreview, setBuildLogPreview] = useState<CleanupPreview | null>(null);
  const [auditResult, setAuditResult] = useState<CleanupResult | null>(null);
  const [buildLogResult, setBuildLogResult] = useState<CleanupResult | null>(null);
  const [pendingDelete, setPendingDelete] = useState<"audit" | "build-logs" | null>(null);

  const auditPreviewMutation = useMutation({
    mutationFn: ({ page, snapshotBefore }: { page: number; snapshotBefore?: number }) =>
      cleanupApi.previewAudit({
        ...auditFilters(audit),
        page,
        per_page: 25,
        snapshot_before: snapshotBefore,
      }),
    onSuccess: (value) => {
      setAuditPreview(value);
      setAuditResult(null);
    },
  });
  const buildLogPreviewMutation = useMutation({
    mutationFn: () => cleanupApi.previewBuildLogs(buildLogFilters(buildLogs)),
    onSuccess: (value) => {
      setBuildLogPreview(value);
      setBuildLogResult(null);
    },
  });
  const auditDeleteMutation = useMutation({
    mutationFn: () =>
      cleanupApi.deleteAudit({
        ...auditFilters(audit),
        snapshot_before: auditPreview?.snapshot_before,
      }),
    onSuccess: (value) => {
      setAuditResult(value);
      setAuditPreview(null);
    },
  });
  const buildLogDeleteMutation = useMutation({
    mutationFn: () => cleanupApi.deleteBuildLogs(buildLogFilters(buildLogs)),
    onSuccess: (value) => {
      setBuildLogResult(value);
      setBuildLogPreview(null);
    },
  });

  function confirmDelete() {
    if (pendingDelete === "audit") auditDeleteMutation.mutate();
    if (pendingDelete === "build-logs") buildLogDeleteMutation.mutate();
    setPendingDelete(null);
  }

  return (
    <>
      <div className="flex flex-col gap-6">
        <div>
          <h2 className="text-lg font-semibold">Cleanup</h2>
          <p className="text-sm text-muted-foreground">
            Review and remove persisted audit events or build logs using server-side filters.
          </p>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>Audit events</CardTitle>
            <CardDescription>
              Filter by time, action, actor, target, team, and result. Cleanup is permanent.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <form
              className="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                auditPreviewMutation.mutate({ page: 1 });
              }}
            >
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                <Input
                  placeholder="Action prefix"
                  value={audit.action}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, action: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events by action"
                />
                <Input
                  placeholder="Actor user ID"
                  value={audit.actorUserId}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, actorUserId: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events by actor user ID"
                />
                <Select
                  value={audit.actorType}
                  onValueChange={(value) => {
                    setAudit((current) => ({
                      ...current,
                      actorType: value as AuditActorType | "all",
                    }));
                    setAuditPreview(null);
                  }}
                >
                  <SelectTrigger aria-label="Cleanup audit events by actor type">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="all">All actor types</SelectItem>
                      <SelectItem value="user">User</SelectItem>
                      <SelectItem value="node">Node</SelectItem>
                      <SelectItem value="system">System</SelectItem>
                      <SelectItem value="anonymous">Anonymous</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <Input
                  placeholder="Target type"
                  value={audit.targetType}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, targetType: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events by target type"
                />
                <Input
                  placeholder="Target ID"
                  value={audit.targetId}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, targetId: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events by target ID"
                />
                <Input
                  placeholder="Team ID"
                  value={audit.teamId}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, teamId: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events by team ID"
                />
                <Select
                  value={audit.result}
                  onValueChange={(value) => {
                    setAudit((current) => ({
                      ...current,
                      result: value as AuditResult | "all",
                    }));
                    setAuditPreview(null);
                  }}
                >
                  <SelectTrigger aria-label="Cleanup audit events by result">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="all">All results</SelectItem>
                      <SelectItem value="success">Success</SelectItem>
                      <SelectItem value="failure">Failure</SelectItem>
                      <SelectItem value="denied">Denied</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <Input
                  type="datetime-local"
                  value={audit.from}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, from: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events from time"
                />
                <Input
                  type="datetime-local"
                  value={audit.to}
                  onChange={(event) => {
                    setAudit((current) => ({ ...current, to: event.target.value }));
                    setAuditPreview(null);
                  }}
                  aria-label="Cleanup audit events to time"
                />
              </div>
              <div className="flex flex-wrap items-center gap-3">
                <Button type="submit" variant="outline" disabled={auditPreviewMutation.isPending}>
                  {auditPreviewMutation.isPending ? "Previewing…" : "Preview Audit Events"}
                </Button>
                <Button
                  type="button"
                  variant="destructive"
                  disabled={!auditPreview?.deletable || auditDeleteMutation.isPending}
                  onClick={() => setPendingDelete("audit")}
                >
                  {auditDeleteMutation.isPending ? "Deleting…" : "Delete Audit Events"}
                </Button>
                <PreviewSummary preview={auditPreview} />
              </div>
              <ResultMessage result={auditResult} />
              <AuditEventResults
                events={auditPreview?.events}
                pagination={auditPreview?.pagination}
                isLoading={auditPreviewMutation.isPending}
                emptyMessage="No audit events match this cleanup filter."
                onPageChange={(page) =>
                  auditPreviewMutation.mutate({
                    page,
                    snapshotBefore: auditPreview?.snapshot_before,
                  })
                }
              />
            </form>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Build logs</CardTitle>
            <CardDescription>
              Remove persisted deployment build logs. Active, pending, assigned, and migrating
              deployments are protected.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <form
              className="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                buildLogPreviewMutation.mutate();
              }}
            >
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                <Input
                  placeholder="Deployment ID"
                  value={buildLogs.deploymentId}
                  onChange={(event) => {
                    setBuildLogs((current) => ({ ...current, deploymentId: event.target.value }));
                    setBuildLogPreview(null);
                  }}
                  aria-label="Cleanup build logs by deployment ID"
                />
                <Input
                  placeholder="Project ID"
                  value={buildLogs.projectId}
                  onChange={(event) => {
                    setBuildLogs((current) => ({ ...current, projectId: event.target.value }));
                    setBuildLogPreview(null);
                  }}
                  aria-label="Cleanup build logs by project ID"
                />
                <Input
                  placeholder="Team ID"
                  value={buildLogs.teamId}
                  onChange={(event) => {
                    setBuildLogs((current) => ({ ...current, teamId: event.target.value }));
                    setBuildLogPreview(null);
                  }}
                  aria-label="Cleanup build logs by team ID"
                />
                <Input
                  placeholder="Triggered by user ID"
                  value={buildLogs.triggeredByUserId}
                  onChange={(event) => {
                    setBuildLogs((current) => ({
                      ...current,
                      triggeredByUserId: event.target.value,
                    }));
                    setBuildLogPreview(null);
                  }}
                  aria-label="Cleanup build logs by user ID"
                />
                <Input
                  type="datetime-local"
                  value={buildLogs.from}
                  onChange={(event) => {
                    setBuildLogs((current) => ({ ...current, from: event.target.value }));
                    setBuildLogPreview(null);
                  }}
                  aria-label="Cleanup build logs from time"
                />
                <Input
                  type="datetime-local"
                  value={buildLogs.to}
                  onChange={(event) => {
                    setBuildLogs((current) => ({ ...current, to: event.target.value }));
                    setBuildLogPreview(null);
                  }}
                  aria-label="Cleanup build logs to time"
                />
              </div>
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  type="submit"
                  variant="outline"
                  disabled={buildLogPreviewMutation.isPending}
                >
                  {buildLogPreviewMutation.isPending ? "Previewing…" : "Preview Build Logs"}
                </Button>
                <Button
                  type="button"
                  variant="destructive"
                  disabled={!buildLogPreview?.deletable || buildLogDeleteMutation.isPending}
                  onClick={() => setPendingDelete("build-logs")}
                >
                  {buildLogDeleteMutation.isPending ? "Deleting…" : "Delete Build Logs"}
                </Button>
                <PreviewSummary preview={buildLogPreview} />
              </div>
              <ResultMessage result={buildLogResult} />
            </form>
          </CardContent>
        </Card>
      </div>

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => !open && setPendingDelete(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete {pendingDelete === "audit" ? "audit events" : "build logs"}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This permanently removes every deletable record matching the current filters.
              Protected build logs will be skipped.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
