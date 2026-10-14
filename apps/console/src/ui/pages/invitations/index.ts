import { QueryController } from "@/infra/query";
import { session } from "@/services/auth/state";
import { teamsApi } from "@/services/teams";
import { refreshTeams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { Page, details, heading } from "../base";
export class InvitationPage extends Page {
  private token = new URLSearchParams(window.location.search).get("token") || "";
  private invitation = new QueryController(this, {
    queryKey: ["invitation", this.token, session.value.user?.id],
    queryFn: () => teamsApi.preflightInvitation(this.token),
    enabled: Boolean(this.token),
    retry: false,
  });
  render() {
    this.classList.add("page");
    if (!this.token)
      return html`<gw-alert tone="error">The invitation link is missing its token.</gw-alert>`;
    const state = this.queryState(this.invitation);
    if (state) return state;
    const invitation = this.invitation.result.data!;
    const destination = `/invitations/accept?${new URLSearchParams({ token: this.token })}`;
    return html`${heading("You're invited", `Join ${invitation.team.name}.`)}<gw-card>${details({ team: invitation.team.name, role: invitation.role, expires: invitation.expires_at })}${
      invitation.status !== "pending"
        ? html`<gw-alert tone="warning">${invitation.status.replaceAll("_", " ")}</gw-alert>`
        : !session.value.user
          ? html`<div class="form-footer">
              <gw-button href=${`/login?${new URLSearchParams({ return_to: destination })}`}
                >Sign in</gw-button
              ><gw-button
                variant="outline"
                href=${`/signup?${new URLSearchParams({ return_to: destination })}`}
                >Create account</gw-button
              >
            </div>`
          : invitation.can_accept
            ? html`<div class="form-footer">
                <gw-button
                  .loading=${this.busy}
                  @click=${() =>
                    this.run(
                      () => teamsApi.acceptInvitation(this.token),
                      async (result) => {
                        await refreshTeams(result.member.team_id);
                        this.go("/", true);
                      },
                      "Invitation accepted.",
                    )}
                  >Accept invitation</gw-button
                >
              </div>`
            : html`<gw-alert tone="warning"
                >This invitation belongs to a different email address.</gw-alert
              >`
    }</gw-card>${this.error()}`;
  }
}
define("gw-page-invitation", InvitationPage);
