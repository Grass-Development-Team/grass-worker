import { QueryController } from "@/infra/query";
import { teamsApi, type InvitationCandidate, type ManagedTeamRole } from "@/services/teams";
import { canManageMembers } from "@/services/teams/permissions";
import { teams } from "@/services/teams/state";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing } from "lit";
import { Page, badge, field, heading, readForm, table, text } from "../../base";
export class MembersPage extends Page {
  static properties = {
    ...Page.properties,
    candidates: { state: true },
    invitation: { state: true },
  };
  candidates: InvitationCandidate[] = [];
  invitation = "";
  private searchTimer?: ReturnType<typeof setTimeout>;
  private version = 0;
  private team = teams.value.active;
  private members = new QueryController(this, {
    queryKey: ["team", this.team?.id, "members"],
    queryFn: () => teamsApi.listMembers(this.team!.id),
    enabled: Boolean(this.team),
  });
  disconnectedCallback() {
    this.version++;
    clearTimeout(this.searchTimer);
    this.invitation = "";
    super.disconnectedCallback();
  }
  private search(value: string) {
    clearTimeout(this.searchTimer);
    const version = ++this.version;
    this.searchTimer = setTimeout(async () => {
      try {
        const result = await teamsApi.invitationCandidates(this.team!.id, value);
        if (version === this.version) this.candidates = result.candidates;
      } catch {
        if (version === this.version) this.candidates = [];
      }
    }, 250);
  }
  render() {
    this.classList.add("page");
    if (!this.team) return html`<gw-empty-state heading="No team selected"></gw-empty-state>`;
    const state = this.queryState(this.members);
    if (state) return state;
    const members = this.members.result.data!.members,
      canManage = Boolean(teams.value.role && canManageMembers(teams.value.role)),
      editing = this.context.params.userId,
      inviting = this.context.path.endsWith("/new"),
      member = members.find((member) => member.user_id === editing);
    const roles = ["member", "admin", "viewer"].map((value) => ({ value, label: value }));
    if (inviting || editing)
      return html`${heading(inviting ? "Invite a member" : "Member settings", "Manage access to this team.", html`<gw-button variant="ghost" href="/settings/members">Back to members</gw-button>`)}${this.error()}${
        !canManage
          ? html`<gw-alert tone="warning"
              >Only team owners and admins can manage members.</gw-alert
            >`
          : editing && !member
            ? html`<gw-alert tone="warning">Member not found.</gw-alert>`
            : html`<gw-card
                ><form
                  class="form"
                  @submit=${(event: Event) => {
                    const data = readForm(event);
                    void this.run(
                      async () =>
                        inviting
                          ? teamsApi.inviteMember(this.team!.id, {
                              email: text(data, "email"),
                              role: text(data, "role") as ManagedTeamRole,
                            })
                          : teamsApi.updateMemberRole(
                              this.team!.id,
                              editing,
                              text(data, "role") as ManagedTeamRole,
                            ),
                      async (result) => {
                        await this.members.refresh();
                        if ("invitation" in result)
                          this.invitation = `${window.location.origin}/invitations/accept?${new URLSearchParams({ token: result.invitation.token })}`;
                        else this.go("/settings/members");
                      },
                      inviting ? "Invitation created." : "Member updated.",
                    );
                  }}
                >
                  ${inviting ? html`<gw-combobox name="email" label="Email" required placeholder="Search by name or email" .options=${this.candidates.map((candidate) => ({ value: candidate.email, label: candidate.display_name || candidate.email }))} @gw-input=${(event: CustomEvent<{ value: string }>) => this.search(event.detail.value)}></gw-combobox>` : html`<p>${member!.display_name || member!.email}</p>`}${
                    member?.role === "owner"
                      ? html`<gw-alert tone="neutral"
                          >The team owner's role cannot be changed.</gw-alert
                        >`
                      : html`${field({ name: "role", label: "Role", options: roles, required: true }, member?.role || "member")}
                          <div class="form-footer">
                            <gw-button type="submit" .loading=${this.busy}
                              >${inviting ? "Create invitation" : "Save role"}</gw-button
                            >
                          </div>`
                  }
                </form>
                ${this.invitation ? html`<div class="stack" style="margin-top:24px"><gw-alert tone="success">Share this invitation link with the member.</gw-alert><code class="code break">${this.invitation}</code><gw-button variant="outline" @click=${() => this.run(() => navigator.clipboard.writeText(this.invitation), undefined, "Invitation link copied.")}>Copy invitation</gw-button></div>` : nothing}</gw-card
              >`
      }`;
    return html`${heading("Team members", "Manage who can access your workspace.", canManage ? html`<gw-button href="/settings/members/new">Invite member</gw-button>` : nothing)}${this.error()}${table(
      members,
      [
        {
          label: "Member",
          render: (member) =>
            html`<strong>${member.display_name || member.email}</strong>
              <div class="muted">${member.email}</div>`,
        },
        { label: "Role", render: (member) => badge(member.role) },
        { label: "Joined", render: (member) => new Date(member.joined_at).toLocaleDateString() },
        {
          label: "Actions",
          render: (member) =>
            canManage && member.role !== "owner"
              ? html`<div class="actions">
                  <gw-button variant="ghost" size="sm" href=${`/settings/members/${member.user_id}`}
                    >Edit</gw-button
                  ><gw-button
                    variant="ghost"
                    size="sm"
                    @click=${async () => {
                      if (
                        await confirmAction(
                          "Remove member?",
                          `${member.email} will lose access to this team.`,
                          "Remove",
                        )
                      )
                        void this.run(
                          () => teamsApi.removeMember(this.team!.id, member.user_id),
                          () => this.members.refresh().then(() => undefined),
                          "Member removed.",
                        );
                    }}
                    >Remove</gw-button
                  >
                </div>`
              : nothing,
        },
      ],
      "Team members",
    )}`;
  }
}
define("gw-page-members", MembersPage);
