import { QueryController } from "@/infra/query";
import { auditApi, type AuditFilters } from "@/services/audit";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { Page, badge, details, field, heading, readForm, table, text } from "../base";
export class AuditPage extends Page {
  static properties = { ...Page.properties, filters: { state: true } };
  filters: AuditFilters = { page: 1 };
  private team = teams.value.active;
  private events = new QueryController(this, {
    queryKey: ["audit", window.location.pathname, this.team?.id, this.filters],
    queryFn: () =>
      window.location.pathname.startsWith("/admin")
        ? auditApi.listAdmin(this.filters)
        : auditApi.listTeam(this.team!.id, this.filters),
  });
  private refresh() {
    this.events.configure({
      queryKey: ["audit", this.context.path, this.team?.id, this.filters],
      queryFn: () =>
        this.context.path.startsWith("/admin")
          ? auditApi.listAdmin(this.filters)
          : auditApi.listTeam(this.team!.id, this.filters),
    });
  }
  render() {
    this.classList.add("page");
    const data = this.events.result.data;
    return html`${heading("Audit events", "Review account, team, project, and platform operations.")}
      <form
        class="grid"
        @submit=${(event: Event) => {
          const form = readForm(event);
          this.filters = {
            action: text(form, "action") || undefined,
            actor_type: (text(form, "actor_type") || undefined) as AuditFilters["actor_type"],
            target_type: text(form, "target_type") || undefined,
            target_id: text(form, "target_id") || undefined,
            result: (text(form, "result") || undefined) as AuditFilters["result"],
            from: text(form, "since") ? new Date(text(form, "since")).getTime() : undefined,
            to: text(form, "until") ? new Date(text(form, "until")).getTime() : undefined,
            page: 1,
          };
          this.refresh();
        }}
      >
        ${field({ name: "action", label: "Action" })}${field({ name: "actor_type", label: "Actor", options: [{ value: "", label: "All actors" }, ...["user", "node", "system", "anonymous"].map((value) => ({ value, label: value }))] })}${field({ name: "target_type", label: "Resource type" })}${field({ name: "target_id", label: "Resource ID" })}${field({ name: "result", label: "Result", options: [{ value: "", label: "All results" }, ...["success", "failure", "denied"].map((value) => ({ value, label: value }))] })}${field({ name: "since", label: "Since", type: "datetime-local" })}${field({ name: "until", label: "Until", type: "datetime-local" })}
        <div class="form-footer">
          <gw-button type="submit" variant="outline">Apply filters</gw-button>
        </div>
      </form>
      ${
        this.queryState(this.events) ??
        html`${table(
            data!.events,
            [
              { label: "Time", key: "created_at" },
              { label: "Action", key: "action" },
              { label: "Actor", render: (event) => event.actor_type },
              {
                label: "Resource",
                render: (event) => `${event.target_type} ${event.target_id ?? ""}`,
              },
              { label: "Result", render: (event) => badge(event.result) },
              {
                label: "Details",
                render: (event) =>
                  html`<details>
                    <summary>Inspect</summary>
                    ${details({ reason: event.reason, actor_user: event.actor_user_id, actor_node: event.actor_node_id, request_id: event.request_id, changes: event.changes, metadata: event.metadata })}
                  </details>`,
              },
            ],
            "Audit events",
          )}<gw-pagination
            .page=${data!.pagination.page}
            .pages=${data!.pagination.total_pages}
            @gw-page=${(event: CustomEvent<{ page: number }>) => {
              this.filters = { ...this.filters, page: event.detail.page };
              this.refresh();
            }}
          ></gw-pagination>`
      }`;
  }
}
define("gw-page-audit", AuditPage);
