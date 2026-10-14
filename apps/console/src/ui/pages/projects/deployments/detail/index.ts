import { QueryController } from "@/infra/query";
import {
  deploymentRefetchInterval,
  deploymentsApi,
  formatDuration,
  isBuildRunning,
} from "@/services/deployments";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import "@/ui/widgets/deployment-log";
import { html, nothing, type PropertyValues } from "lit";
import { badge, details, heading, table } from "../../../base";
import { ProjectPage } from "../../shared";
export class DeploymentDetailPage extends ProjectPage {
  private detail = new QueryController(this, {
    queryKey: ["deployment", "none"],
    queryFn: () => deploymentsApi.detail(this.projectId, this.context.params.deploymentId),
    enabled: false,
  });
  protected willUpdate(changed: PropertyValues) {
    super.willUpdate(changed);
    if (changed.has("context"))
      this.detail.configure({
        queryKey: ["deployment", this.projectId, this.context.params.deploymentId],
        queryFn: () => deploymentsApi.detail(this.projectId, this.context.params.deploymentId),
        enabled: Boolean(this.projectId),
        refetchInterval: (query) => deploymentRefetchInterval(query.state.data?.deployment),
      });
  }
  private action(operation: () => Promise<unknown>, message: string) {
    void this.run(operation, () => this.detail.refresh().then(() => undefined), message);
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.detail) ?? this.queryState(this.project);
    if (state) return state;
    const data = this.detail.result.data!,
      deployment = data.deployment,
      running = isBuildRunning(deployment.build_status),
      ready = deployment.build_status === "ready",
      canRelease =
        ready &&
        ["ready", "retired"].includes(deployment.serve_status) &&
        !deployment.release_pending;
    return html`${heading(deployment.source.commit_message || "Deployment", `${deployment.environment} · ${deployment.source.branch || ""}`, html`${deployment.preview_url ? html`<gw-button variant="outline" href=${deployment.preview_url}>Open preview</gw-button>` : nothing}${deployment.production_url ? html`<gw-button variant="outline" href=${deployment.production_url}>Visit site</gw-button>` : nothing}`)}
      <div class="row">
        ${badge(deployment.build_status)}${badge(deployment.serve_status)}${badge(deployment.release_status)}${deployment.release_pending ? badge("pending release") : nothing}
      </div>
      ${this.error()}${deployment.failure_message || deployment.serve_failure_message ? html`<gw-alert tone="error">${deployment.failure_message || deployment.serve_failure_message}</gw-alert>` : nothing}${deployment.release_status === "pending_review" ? html`<gw-alert tone="info">This deployment is waiting for approval.</gw-alert>` : nothing}
      <div class="actions">
        ${
          running && this.canEdit
            ? html`<gw-button
                variant="outline"
                ?disabled=${this.busy}
                @click=${async () => {
                  if (
                    await confirmAction(
                      "Cancel build?",
                      "The running build will be stopped.",
                      "Cancel build",
                    )
                  )
                    this.action(
                      () => deploymentsApi.cancel(this.projectId, deployment.id),
                      "Build canceled.",
                    );
                }}
                >Cancel build</gw-button
              >`
            : nothing
        }${["failed", "canceled"].includes(deployment.build_status) && this.canEdit ? html`<gw-button ?disabled=${this.busy} @click=${() => this.action(() => deploymentsApi.retry(this.projectId, deployment.id), "Build retried.")}>Retry</gw-button>` : nothing}${ready && data.review_required && deployment.release_status === "draft" && this.canManage ? html`<gw-button ?disabled=${this.busy} @click=${() => this.action(() => deploymentsApi.promote(this.projectId, deployment.id), "Submitted for review.")}>Submit for review</gw-button>` : nothing}${ready && deployment.release_status !== "active" && (!data.review_required || deployment.release_status === "approved") && this.canManage ? html`<gw-button ?disabled=${this.busy || !canRelease} @click=${() => this.action(() => deploymentsApi.promote(this.projectId, deployment.id), "Release requested.")}>Promote</gw-button>` : nothing}${ready && deployment.release_status === "approved" && data.was_active && this.canManage ? html`<gw-button variant="outline" ?disabled=${this.busy || !canRelease} @click=${() => this.action(() => deploymentsApi.rollback(this.projectId, deployment.id), "Rollback requested.")}>Rollback</gw-button>` : nothing}${
          deployment.release_status === "active" && this.canManage
            ? html`<gw-button
                variant="danger"
                ?disabled=${this.busy}
                @click=${async () => {
                  if (
                    await confirmAction(
                      "Unpublish deployment?",
                      "The active release will no longer be served.",
                      "Unpublish",
                    )
                  )
                    this.action(
                      () => deploymentsApi.unpublish(this.projectId, deployment.id),
                      "Deployment unpublished.",
                    );
                }}
                >Unpublish</gw-button
              >`
            : nothing
        }
      </div>
      <gw-card heading="Build log"
        ><gw-deployment-log
          .projectId=${this.projectId}
          .deploymentId=${deployment.id}
          .status=${deployment.build_status}
          @gw-done=${() => this.detail.refresh()}
        ></gw-deployment-log
      ></gw-card>
      <div class="grid">
        <gw-card heading="Deployment details"
          >${details({ id: deployment.id, region: deployment.region, runtime: deployment.runtime_kind, build_node: deployment.build_node?.name, serve_node: deployment.serve_node?.name, duration: formatDuration(deployment.duration_seconds), commit: deployment.source.commit_hash, triggered_by: deployment.triggered_by?.display_name || deployment.triggered_by?.email, resources: deployment.serve_resources, overcommitted: deployment.overcommitted })}</gw-card
        ><gw-card heading="Screenshot"
          >${deployment.screenshot_url ? html`<gw-image src=${deployment.screenshot_url} alt="Deployment screenshot"></gw-image>` : html`<p>${deployment.screenshot_status === "pending" ? "Screenshot is being captured." : "No screenshot is available."}</p>`}</gw-card
        >
      </div>
      <gw-card heading="Activity"
        >${table(
          data.events,
          [
            { label: "Time", key: "created_at" },
            { label: "Kind", key: "kind" },
            { label: "Message", key: "message" },
          ],
          "Deployment activity",
        )}</gw-card
      ><gw-card heading="Artifacts"
        >${table(
          data.artifacts,
          [
            { label: "Kind", key: "kind" },
            {
              label: "Size",
              render: (artifact) =>
                artifact.size_bytes === null ? "—" : `${artifact.size_bytes} bytes`,
            },
            {
              label: "Checksum",
              render: (artifact) => html`<code>${artifact.checksum_sha256 || "—"}</code>`,
            },
          ],
          "Deployment artifacts",
        )}</gw-card
      ><gw-card heading="Reviews"
        >${table(
          data.reviews,
          [
            { label: "Status", render: (review) => badge(review.status) },
            { label: "Reason", key: "reason" },
            { label: "Reviewed", key: "reviewed_at" },
          ],
          "Deployment reviews",
        )}</gw-card
      >`;
  }
}
define("gw-page-deployment-detail", DeploymentDetailPage);
