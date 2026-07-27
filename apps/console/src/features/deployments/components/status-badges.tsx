import { Badge } from "@/components/ui/badge";

import type { BuildStatus, ReleaseStatus, ServeStatus } from "../deployments.api";

export function BuildStatusBadge({ status }: { status: BuildStatus }) {
  switch (status) {
    case "ready":
      return <Badge variant="success">Ready</Badge>;
    case "failed":
      return <Badge variant="destructive">Failed</Badge>;
    case "canceled":
      return <Badge variant="secondary">Canceled</Badge>;
    case "building":
      return <Badge variant="warning">Building</Badge>;
    case "queued":
    case "claimed":
      return <Badge variant="warning">{status === "queued" ? "Queued" : "Claimed"}</Badge>;
    default:
      return <Badge variant="outline">Pending</Badge>;
  }
}

export function ServeStatusBadge({ status }: { status: ServeStatus }) {
  switch (status) {
    case "ready":
      return <Badge variant="success">Serve ready</Badge>;
    case "failed":
      return <Badge variant="destructive">Serve failed</Badge>;
    case "syncing":
      return <Badge variant="warning">Syncing</Badge>;
    default:
      return <Badge variant="outline">Serve pending</Badge>;
  }
}

export function ReleaseStatusBadge({ status }: { status: ReleaseStatus }) {
  switch (status) {
    case "active":
      return <Badge variant="success">Active</Badge>;
    case "approved":
      return <Badge variant="outline">Approved</Badge>;
    case "pending_review":
      return <Badge variant="warning">Pending review</Badge>;
    case "rejected":
      return <Badge variant="destructive">Rejected</Badge>;
    default:
      return <Badge variant="secondary">Draft</Badge>;
  }
}
