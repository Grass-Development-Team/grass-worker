import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, ExternalLinkIcon, InboxIcon, RocketIcon, XIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import { adminApi, type AdminReview } from "../admin.api";

function environmentBadge(environment: "production" | "preview") {
  return environment === "production" ? (
    <Badge>Production</Badge>
  ) : (
    <Badge variant="secondary">Preview</Badge>
  );
}

function serveBadge(status: AdminReview["deployment"]["serve_status"]) {
  switch (status) {
    case "ready":
      return <Badge variant="success">Ready</Badge>;
    case "failed":
      return <Badge variant="destructive">Failed</Badge>;
    case "syncing":
      return <Badge variant="warning">Syncing</Badge>;
    case "retired":
      return <Badge variant="secondary">Retired</Badge>;
    default:
      return <Badge variant="outline">Pending</Badge>;
  }
}

export function ReviewsPanel() {
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [rejecting, setRejecting] = useState<AdminReview | null>(null);

  const reviewsQuery = useQuery({
    queryKey: ["admin", "reviews"],
    queryFn: adminApi.listReviews,
    refetchInterval: 30_000,
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["admin", "reviews"] });
    queryClient.invalidateQueries({ queryKey: ["deployments"] });
  };

  const approveMutation = useMutation({
    mutationFn: ({ deploymentId, promote }: { deploymentId: string; promote: boolean }) =>
      adminApi.approveReview(deploymentId, { promote }),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to approve the deployment."),
  });

  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        Deployments waiting for release review across all teams. Approve unblocks the team's Promote
        button; Approve &amp; promote publishes after the target is ready on a Serve Node.
      </p>

      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {reviewsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {reviewsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {reviewsQuery.error instanceof Error
            ? reviewsQuery.error.message
            : "Unable to load pending reviews."}
        </p>
      )}

      {reviewsQuery.data &&
        (reviewsQuery.data.reviews.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <InboxIcon className="mr-1 inline size-4" />
            No deployments are waiting for review.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Project</TableHead>
                <TableHead>Environment</TableHead>
                <TableHead>Serve</TableHead>
                <TableHead>Commit</TableHead>
                <TableHead>Requested</TableHead>
                <TableHead className="text-right">Decision</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {reviewsQuery.data.reviews.map((review) => {
                const decisionsReady =
                  review.deployment.serve_status === "ready" ||
                  (review.deployment.serve_status === "retired" &&
                    review.deployment.serve_was_ready);
                return (
                  <TableRow key={review.id}>
                    <TableCell>
                      <span className="font-medium">{review.project.name}</span>
                      <p className="text-xs text-muted-foreground">
                        {review.team ? `${review.team.name} · ` : ""}
                        {review.project.slug}
                      </p>
                    </TableCell>
                    <TableCell>{environmentBadge(review.deployment.environment)}</TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        {serveBadge(review.deployment.serve_status)}
                        {review.deployment.preview_host &&
                          review.deployment.serve_status !== "retired" && (
                            <a
                              href={`//${review.deployment.preview_host}`}
                              target="_blank"
                              rel="noreferrer"
                              className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                            >
                              Open preview <ExternalLinkIcon className="size-3" />
                            </a>
                          )}
                      </div>
                    </TableCell>
                    <TableCell className="max-w-56">
                      <p className="truncate text-sm">
                        {review.deployment.commit_message ?? "No commit message"}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {review.deployment.source_branch ?? "default branch"}
                        {review.deployment.commit_hash
                          ? ` · ${review.deployment.commit_hash.slice(0, 7)}`
                          : ""}
                      </p>
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {new Date(review.requested_at).toLocaleString()}
                      {review.triggered_by && (
                        <p className="text-xs">{review.triggered_by.email}</p>
                      )}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() =>
                            approveMutation.mutate({
                              deploymentId: review.deployment.id,
                              promote: false,
                            })
                          }
                          disabled={!decisionsReady || approveMutation.isPending}
                        >
                          <CheckIcon /> Approve
                        </Button>
                        <Button
                          size="sm"
                          onClick={() =>
                            approveMutation.mutate({
                              deploymentId: review.deployment.id,
                              promote: true,
                            })
                          }
                          disabled={!decisionsReady || approveMutation.isPending}
                        >
                          <RocketIcon /> Approve &amp; promote
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => setRejecting(review)}
                          disabled={!decisionsReady || approveMutation.isPending}
                        >
                          <XIcon /> Reject
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        ))}

      <RejectDialog
        review={rejecting}
        onClose={() => setRejecting(null)}
        onRejected={() => {
          setRejecting(null);
          setError(null);
          invalidate();
        }}
      />
    </div>
  );
}

function RejectDialog({
  review,
  onClose,
  onRejected,
}: {
  review: AdminReview | null;
  onClose: () => void;
  onRejected: () => void;
}) {
  const [reason, setReason] = useState("");

  const rejectMutation = useMutation({
    mutationFn: () =>
      adminApi.rejectReview(review?.deployment.id ?? "", reason.trim() || undefined),
    onSuccess: () => {
      setReason("");
      onRejected();
    },
  });

  return (
    <Dialog open={review !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Reject deployment</DialogTitle>
          <DialogDescription>
            {review
              ? `Reject the ${review.deployment.environment} deployment of ${review.project.name}. The team can fix it and retry the deployment; a new review is created after the build is ready.`
              : ""}
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (review) rejectMutation.mutate();
          }}
        >
          <Field>
            <FieldLabel htmlFor="reject-reason">Reason (optional)</FieldLabel>
            <Input
              id="reject-reason"
              placeholder="Broken layout on the landing page"
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          </Field>
          {rejectMutation.isError && (
            <p role="alert" className="text-sm text-destructive">
              {rejectMutation.error instanceof Error
                ? rejectMutation.error.message
                : "Unable to reject the deployment."}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={rejectMutation.isPending}>
              {rejectMutation.isPending ? "Rejecting…" : "Reject"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
