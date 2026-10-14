import { StoreController } from "@/infra/controllers/store";
import { authApi } from "@/services/auth";
import { commitSession, session } from "@/services/auth/state";
import { define } from "@/ui/components/base";
import "@/ui/widgets/avatar-editor";
import { html } from "lit";
import { Page, field, heading, readForm, text } from "../../base";
export class ProfilePage extends Page {
  private auth = new StoreController(this, session);
  render() {
    this.classList.add("page");
    const user = this.auth.value.user!;
    return html`${heading("Personal settings", "Manage your profile and account information.")}
      <nav class="page-tabs">
        <a href="/account/profile" aria-current="page">Profile</a
        ><a href="/account/security">Security</a>
      </nav>
      ${this.error()}<gw-card heading="Profile"
        ><form
          class="form"
          @submit=${(event: Event) => {
            const data = readForm(event);
            void this.run(
              () => authApi.updateMe({ display_name: text(data, "display_name") || null }),
              (result) => commitSession(result.user),
            );
          }}
        >
          ${field({ name: "display_name", label: "Display name" }, user.display_name)}${field({ name: "email", label: "Email", type: "email", readonly: true }, user.email)}
          <div class="row">
            <gw-badge tone=${user.email_verified ? "success" : "warning"}
              >${user.email_verified ? "Email verified" : "Email not verified"}</gw-badge
            >${!user.email_verified ? html`<gw-button variant="ghost" @click=${() => this.run(() => authApi.resendVerification(user.email), undefined, "Verification email sent.")}>Send verification email</gw-button>` : ""}
          </div>
          <div class="form-footer">
            <gw-button type="submit" .loading=${this.busy}>Save profile</gw-button>
          </div>
        </form></gw-card
      ><gw-card heading="Avatar"
        ><gw-avatar-editor
          src=${user.avatar_url || ""}
          label=${user.display_name || user.email}
          .busy=${this.busy}
          @gw-avatar=${(event: CustomEvent<{ png: Blob }>) =>
            this.run(
              () => authApi.uploadAvatar(event.detail.png),
              (result) => commitSession(result.user),
            )}
          @gw-avatar-remove=${() => this.run(authApi.deleteAvatar, (result) => commitSession(result.user))}
        ></gw-avatar-editor
      ></gw-card>`;
  }
}
define("gw-page-profile", ProfilePage);
