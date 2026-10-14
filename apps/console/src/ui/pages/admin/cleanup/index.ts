import {
  cleanupApi,
  type AuditCleanupFilters,
  type AuditCleanupPreview,
  type BuildLogCleanupFilters,
  type CleanupPreview,
  type CleanupResult,
} from "@/services/cleanup";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing } from "lit";
import { Page, details, field, heading, readForm, table, text } from "../../base";
export class CleanupPage extends Page {
  static properties = {
    ...Page.properties,
    kind: { state: true },
    preview: { state: true },
    result: { state: true },
  };
  kind = "audit";
  preview: CleanupPreview | AuditCleanupPreview | null = null;
  result: CleanupResult | null = null;
  private filters: AuditCleanupFilters | BuildLogCleanupFilters = {};
  private previewRevision = 0;
  private invalidate() {
    this.previewRevision++;
    this.preview = null;
    this.result = null;
  }
  render() {
    this.classList.add("page");
    return html`${heading("Data cleanup", "Preview exactly what can be removed before deleting historical data.")}${this.error()}<gw-radio-group
        label="Data type"
        .value=${this.kind}
        .options=${[
          { value: "audit", label: "Audit events" },
          { value: "build-logs", label: "Build logs" },
        ]}
        @gw-change=${(event: CustomEvent<{ value: string }>) => {
          this.kind = event.detail.value;
          this.invalidate();
        }}
      ></gw-radio-group
      ><gw-card heading="Cleanup filters"
        ><form
          class="form"
          @gw-input=${() => this.invalidate()}
          @gw-change=${() => this.invalidate()}
          @submit=${(event: Event) => {
            const form = readForm(event);
            const common = {
              team_id: text(form, "team_id") || undefined,
              from: text(form, "from") ? new Date(text(form, "from")).getTime() : undefined,
              to: text(form, "to") ? new Date(text(form, "to")).getTime() : undefined,
            };
            this.filters =
              this.kind === "audit"
                ? {
                    ...common,
                    action: text(form, "action") || undefined,
                    actor_user_id: text(form, "actor_user_id") || undefined,
                    actor_type: (text(form, "actor_type") ||
                      undefined) as AuditCleanupFilters["actor_type"],
                    target_type: text(form, "target_type") || undefined,
                    target_id: text(form, "target_id") || undefined,
                    result: (text(form, "result") || undefined) as AuditCleanupFilters["result"],
                  }
                : {
                    ...common,
                    project_id: text(form, "project_id") || undefined,
                    deployment_id: text(form, "deployment_id") || undefined,
                    triggered_by_user_id: text(form, "triggered_by_user_id") || undefined,
                  };
            const version = ++this.previewRevision;
            const kind = this.kind;
            const filters = { ...this.filters };
            void this.run(
              () =>
                kind === "audit"
                  ? cleanupApi.previewAudit(filters as AuditCleanupFilters)
                  : cleanupApi.previewBuildLogs(filters as BuildLogCleanupFilters),
              (preview) => {
                if (version !== this.previewRevision || kind !== this.kind) return;
                this.preview = preview;
                if ("snapshot_before" in preview && typeof preview.snapshot_before === "number")
                  this.filters = { ...this.filters, snapshot_before: preview.snapshot_before };
              },
              "",
            );
          }}
        >
          <div class="grid">
            ${field({ name: "team_id", label: "Team ID" })}${field({ name: "from", label: "From", type: "datetime-local" })}${field({ name: "to", label: "To", type: "datetime-local" })}${this.kind === "audit" ? html`${field({ name: "action", label: "Action" })}${field({ name: "actor_user_id", label: "Actor user ID" })}${field({ name: "actor_type", label: "Actor type", options: [{ value: "", label: "All actors" }, ...["user", "node", "system", "anonymous"].map((value) => ({ value, label: value }))] })}${field({ name: "target_type", label: "Target type" })}${field({ name: "target_id", label: "Target ID" })}${field({ name: "result", label: "Result", options: [{ value: "", label: "All results" }, ...["success", "failure", "denied"].map((value) => ({ value, label: value }))] })}` : html`${field({ name: "project_id", label: "Project ID" })}${field({ name: "deployment_id", label: "Deployment ID" })}${field({ name: "triggered_by_user_id", label: "Triggered by user ID" })}`}
          </div>
          <div class="form-footer">
            <gw-button type="submit" variant="outline" .loading=${this.busy}
              >Preview cleanup</gw-button
            >
          </div>
        </form></gw-card
      >${
        this.preview
          ? html`<gw-card heading="Preview"
              >${details({ matched: this.preview.matched, deletable: this.preview.deletable, skipped: this.preview.skipped })}${
                "events" in this.preview
                  ? table(
                      this.preview.events,
                      [
                        { label: "Time", key: "created_at" },
                        { label: "Action", key: "action" },
                        { label: "Result", key: "result" },
                      ],
                      "Audit cleanup preview",
                    )
                  : nothing
              }
              <div class="form-footer">
                <gw-button
                  variant="danger"
                  ?disabled=${!this.preview.deletable || this.busy}
                  @click=${async () => {
                    const filters = { ...this.filters },
                      kind = this.kind;
                    if (
                      await confirmAction(
                        "Delete previewed data?",
                        `${this.preview?.deletable || 0} records are eligible. This operation cannot be undone.`,
                        "Delete data",
                      )
                    )
                      void this.run(
                        () =>
                          kind === "audit"
                            ? cleanupApi.deleteAudit(filters as AuditCleanupFilters)
                            : cleanupApi.deleteBuildLogs(filters as BuildLogCleanupFilters),
                        (result) => {
                          this.result = result;
                          this.preview = null;
                        },
                        "Cleanup completed.",
                      );
                  }}
                  >Delete eligible data</gw-button
                >
              </div></gw-card
            >`
          : nothing
      }${this.result ? html`<gw-card heading="Cleanup result">${details({ deleted: this.result.deleted, skipped: this.result.skipped, failed: this.result.failed || 0 })}</gw-card>` : nothing}`;
  }
}
define("gw-page-admin-cleanup", CleanupPage);
