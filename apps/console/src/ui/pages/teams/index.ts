import { teamsApi } from "@/services/teams";
import { canEditTeam, canManageMembers } from "@/services/teams/permissions";
import { refreshTeams, teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import "@/ui/widgets/avatar-editor";
import { html, nothing } from "lit";
import { Page, details, field, heading, readForm, text } from "../base";
export class TeamPage extends Page {
  render() {
    this.classList.add("page");
    const creating = this.context.path === "/teams/new",
      team = teams.value.active,
      role = teams.value.role,
      editable = creating || Boolean(role && canEditTeam(role));
    if (!creating && !team)
      return html`<gw-empty-state heading="No team selected"
        ><gw-button href="/teams/new">Create team</gw-button></gw-empty-state
      >`;
    return html`${heading(creating ? "Create a team" : "Team settings", creating ? "Create a shared workspace for projects and deployments." : "Manage your team identity and source access.")}${this.error()}<gw-card
        heading=${creating ? "Team details" : "General"}
        ><form
          class="form"
          @submit=${(event: Event) => {
            const data = readForm(event);
            if (!editable) return;
            void this.run(
              () =>
                creating
                  ? teamsApi.create({ name: text(data, "name"), slug: text(data, "slug") })
                  : teamsApi.update(team!.id, {
                      name: text(data, "name"),
                      slug: text(data, "slug"),
                    }),
              async (result) => {
                await refreshTeams(result.team.id);
                if (creating) this.go("/settings/team");
              },
            );
          }}
        >
          ${field({ name: "name", label: "Team name", required: true, readonly: !editable }, team?.name)}${field({ name: "slug", label: "Team slug", required: true, readonly: !editable }, team?.slug)}${editable ? html`<div class="form-footer"><gw-button type="submit" .loading=${this.busy}>${creating ? "Create team" : "Save team"}</gw-button></div>` : html`<p class="muted">Only the team owner can edit these settings.</p>`}
        </form></gw-card
      >${
        !creating && team
          ? html`<gw-card heading="Team avatar"
                >${
                  editable
                    ? html`<gw-avatar-editor
                        src=${team.avatar_url || ""}
                        label=${team.name}
                        .busy=${this.busy}
                        @gw-avatar=${(event: CustomEvent<{ png: Blob }>) =>
                          this.run(
                            () => teamsApi.uploadAvatar(team.id, event.detail.png),
                            () => refreshTeams(team.id),
                          )}
                        @gw-avatar-remove=${() =>
                          this.run(
                            () => teamsApi.deleteAvatar(team.id),
                            () => refreshTeams(team.id),
                          )}
                      ></gw-avatar-editor>`
                    : html`<gw-avatar src=${team.avatar_url || ""} alt=${team.name}></gw-avatar>`
                }</gw-card
              ><gw-card
                heading="Git source access"
                description="Manage private repository credentials and SSH host verification."
                >${role && canManageMembers(role) ? html`<gw-button variant="outline" href="/settings/team/source-access">Manage source access</gw-button>` : html`<p>Ask a team owner or admin to configure private repository access.</p>`}</gw-card
              ><gw-card heading="Team information"
                >${details({ id: team.id, kind: team.kind, role: role, slug: team.slug })}</gw-card
              >`
          : nothing
      }`;
  }
}
define("gw-page-team", TeamPage);
