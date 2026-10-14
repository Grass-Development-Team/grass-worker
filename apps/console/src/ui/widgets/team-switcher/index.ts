import { StoreController } from "@/infra/controllers/store";
import { selectTeam, teams } from "@/services/teams/state";
import { GwElement, define, sharedStyles } from "@/ui/components/base";
import { html } from "lit";
export class TeamSwitcher extends GwElement {
  static styles = sharedStyles;
  private teams = new StoreController(this, teams);
  render() {
    return html`<gw-select
      label="Workspace"
      .value=${this.teams.value.active?.id || ""}
      .options=${this.teams.value.teams.map((team) => ({ value: team.id, label: team.name }))}
      @gw-change=${(event: CustomEvent<{ value: string }>) => selectTeam(event.detail.value)}
    ></gw-select>`;
  }
}
define("gw-team-switcher", TeamSwitcher);
