import { QueryController } from "@/infra/query";
import { adminApi } from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html } from "lit";
import { Page, badge, details, field, heading, table } from "../../base";
export class ReviewsPage extends Page {
  private reviews = new QueryController(this, {
    queryKey: ["admin", "reviews"],
    queryFn: adminApi.listReviews,
    refetchInterval: 10000,
  });
  render() {
    this.classList.add("page");
    const state = this.queryState(this.reviews);
    if (state) return state;
    const reviews = this.reviews.result.data!.reviews,
      id = this.context.params.deploymentId,
      review = reviews.find((review) => review.deployment.id === id);
    if (id) {
      if (!review)
        return html`${heading("Review no longer pending")}<gw-button href="/admin/reviews"
            >Back to reviews</gw-button
          >`;
      return html`${heading(review.project.name, "Review this deployment before it becomes available.", html`<gw-button variant="ghost" href="/admin/reviews">Back to reviews</gw-button>`)}${this.error()}<gw-card
          heading="Deployment"
          >${details({ id: review.deployment.id, environment: review.deployment.environment, branch: review.deployment.source_branch, commit: review.deployment.commit_hash, message: review.deployment.commit_message, team: review.team?.name, triggered_by: review.triggered_by?.email })}
          <div class="row" style="margin-top:16px">
            ${badge(review.deployment.build_status)}${badge(review.deployment.serve_status)}<gw-button
              variant="outline"
              href=${`/projects/${review.project.id}/deployments/${id}`}
              >Inspect deployment</gw-button
            >
          </div></gw-card
        ><gw-card heading="Decision"
          ><div class="stack">
            ${field({ name: "reason", label: "Reason", description: "Include context for the project owner." })}
            <div class="actions">
              <gw-button
                .loading=${this.busy}
                ?disabled=${review.deployment.build_status !== "ready"}
                @click=${() => {
                  const reason = (
                    this.querySelector("gw-input[name='reason']") as HTMLElement & { value: string }
                  ).value.trim();
                  void this.run(
                    () => adminApi.approveReview(id, { ...(reason ? { reason } : {}) }),
                    async () => {
                      await this.reviews.refresh();
                      this.go("/admin/reviews");
                    },
                    "Deployment approved.",
                  );
                }}
                >Approve</gw-button
              ><gw-button
                variant="danger"
                ?disabled=${this.busy}
                @click=${async () => {
                  const reason = (
                    this.querySelector("gw-input[name='reason']") as HTMLElement & { value: string }
                  ).value.trim();
                  if (
                    await confirmAction(
                      "Reject deployment?",
                      "The project owner will receive your decision.",
                      "Reject",
                    )
                  )
                    void this.run(
                      () => adminApi.rejectReview(id, reason || undefined),
                      async () => {
                        await this.reviews.refresh();
                        this.go("/admin/reviews");
                      },
                      "Deployment rejected.",
                    );
                }}
                >Reject</gw-button
              >
            </div>
          </div></gw-card
        >`;
    }
    return html`${heading("Deployment reviews", `${this.reviews.result.data!.total} deployments waiting for review.`)}${table(
      reviews,
      [
        {
          label: "Project",
          render: (review) =>
            html`<a href=${`/admin/reviews/${review.deployment.id}`}>${review.project.name}</a>`,
        },
        { label: "Team", render: (review) => review.team?.name || "—" },
        { label: "Environment", render: (review) => review.deployment.environment },
        { label: "Build", render: (review) => badge(review.deployment.build_status) },
        { label: "Requested", key: "requested_at" },
        {
          label: "Action",
          render: (review) =>
            html`<gw-button
              variant="outline"
              size="sm"
              href=${`/admin/reviews/${review.deployment.id}`}
              >Review</gw-button
            >`,
        },
      ],
      "Pending reviews",
    )}`;
  }
}
define("gw-page-admin-reviews", ReviewsPage);
