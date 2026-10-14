import { QueryController } from "@/infra/query";
import { dimensionLabel, quotaApi } from "@/services/quota";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { Page, heading, table } from "../base";
export class QuotaPage extends Page {
  private team = teams.value.active;
  private usage = new QueryController(this, {
    queryKey: ["team", this.team?.id, "quota"],
    queryFn: () => quotaApi.usage(this.team!.id),
    enabled: Boolean(this.team),
  });
  render() {
    this.classList.add("page");
    if (!this.team)
      return html`<gw-empty-state heading="Select a team to view usage"></gw-empty-state>`;
    const state = this.queryState(this.usage);
    if (state) return state;
    const data = this.usage.result.data!;
    return html`${heading("Usage", `Your ${data.plan.name} plan · ${data.plan.source} assignment`, html`<gw-button variant="outline" @click=${() => this.usage.refresh()}>Refresh</gw-button>`)}${table(
      data.usage,
      [
        { label: "Resource", render: (entry) => dimensionLabel(entry.dimension) },
        { label: "Used", key: "used" },
        { label: "Limit", render: (entry) => entry.limit ?? "Unlimited" },
        { label: "Period", key: "period" },
        {
          label: "Usage",
          render: (entry) =>
            html`<gw-progress
              .value=${entry.limit ? (entry.used / entry.limit) * 100 : 0}
              label=${dimensionLabel(entry.dimension)}
            ></gw-progress>`,
        },
      ],
      "Quota usage",
    )}`;
  }
}
define("gw-page-quota", QuotaPage);
