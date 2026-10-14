import { QueryController } from "@/infra/query";
import { authApi, type MfaFactor, type TotpEnrollment } from "@/services/auth";
import { session } from "@/services/auth/state";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html, nothing } from "lit";
import QRCode from "qrcode";
import { Page, details, field, heading, readForm, secret, text } from "../../base";
export class SecurityPage extends Page {
  static properties = {
    ...Page.properties,
    factor: { state: true },
    enrollment: { state: true },
    qr: { state: true },
  };
  factor: MfaFactor | null = null;
  enrollment: TotpEnrollment | null = null;
  qr = "";
  private security = new QueryController(this, {
    queryKey: ["security", session.value.user?.id],
    queryFn: authApi.security,
  });
  disconnectedCallback() {
    this.factor = null;
    this.enrollment = null;
    this.qr = "";
    super.disconnectedCallback();
  }
  private start(kind: "totp" | "email") {
    void this.run(
      async () => {
        if (kind === "totp") {
          const result = await authApi.accountTotpStart();
          this.enrollment = result;
          this.factor = result.factor;
          this.qr = await QRCode.toDataURL(result.otpauth_uri, { width: 208, margin: 2 });
        } else {
          this.factor = (await authApi.accountEmailStart()).factor;
          this.enrollment = null;
        }
      },
      undefined,
      "",
    );
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.security);
    if (state) return state;
    const data = this.security.result.data!,
      adding = this.context.path.endsWith("/new"),
      password = this.context.path.endsWith("/password");
    const available = data.allowed_factors.filter(
      (kind) =>
        !data.factors.some((factor) => factor.kind === kind && factor.verified) &&
        (kind !== "email" || (data.mail_available && data.email_verified)),
    );
    if (adding)
      return html`${heading("Add a verification method", "Set up a second factor for your account.", html`<gw-button variant="ghost" href="/account/security">Back to security</gw-button>`)}${this.error()}<gw-card
          >${
            !this.factor
              ? html`<div class="grid">
                  ${!available.length ? html`<gw-alert tone="warning">No additional verification method is currently available. Email codes require a verified email address and available email delivery.</gw-alert>` : nothing}${available.map((kind) => html`<gw-button variant="outline" .loading=${this.busy} @click=${() => this.start(kind)}>${kind === "totp" ? "Authenticator app" : "Email code"}</gw-button>`)}
                </div>`
              : html`<div class="stack">
                  ${
                    this.enrollment
                      ? html`<p>
                            Scan the code with your authenticator app, then enter its current
                            six-digit code.
                          </p>
                          <div class="qr">
                            <img
                              src=${this.qr}
                              alt="Authenticator setup QR code"
                              width="208"
                              height="208"
                            />
                          </div>
                          <div class="setup-key">
                            <code>${this.enrollment.secret}</code
                            ><gw-button
                              variant="ghost"
                              @click=${() => this.run(() => navigator.clipboard.writeText(this.enrollment!.secret), undefined, "Setup key copied.")}
                              >Copy key</gw-button
                            >
                          </div>`
                      : html`<p>Enter the code sent to your verified email address.</p>`
                  }
                  <form
                    class="form"
                    @submit=${(event: Event) => {
                      const form = readForm(event);
                      void this.run(
                        () => authApi.accountMfaConfirm(this.factor!.id, text(form, "code")),
                        async () => {
                          await this.security.refresh();
                          this.go("/account/security", true);
                        },
                        "Verification method added.",
                      );
                    }}
                  >
                    <gw-input-otp name="code" required></gw-input-otp>
                    <div class="form-footer">
                      <gw-button
                        variant="outline"
                        @click=${() => {
                          this.factor = null;
                          this.enrollment = null;
                          this.qr = "";
                        }}
                        >Choose another method</gw-button
                      ><gw-button type="submit" .loading=${this.busy}>Verify and add</gw-button>
                    </div>
                  </form>
                </div>`
          }</gw-card
        >`;
    if (password)
      return html`${heading("Change password", "Your existing sessions will end after this change.", html`<gw-button variant="ghost" href="/account/security">Back to security</gw-button>`)}${this.error()}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                async () => {
                  if (secret(form, "password") !== secret(form, "confirm"))
                    throw new Error("Passwords do not match.");
                  return authApi.changePassword(
                    secret(form, "current_password"),
                    secret(form, "password"),
                  );
                },
                undefined,
                "Password changed. Sign in again.",
              );
            }}
          >
            ${field({ name: "current_password", label: "Current password", type: "password", required: true, autocomplete: "current-password" })}<gw-password-input
              name="password"
              label="New password"
              required
              autocomplete="new-password"
              .minLength=${data.password_policy.min_length}
              .maxLength=${data.password_policy.max_length}
            ></gw-password-input
            >${field({ name: "confirm", label: "Confirm password", type: "password", required: true })}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save password</gw-button>
            </div>
          </form></gw-card
        >`;
    return html`${heading("Account security", "Control your password and verification methods.")}
      <nav class="page-tabs">
        <a href="/account/profile">Profile</a
        ><a href="/account/security" aria-current="page">Security</a>
      </nav>
      ${this.error()}<gw-card
        heading="Password"
        description="Use a strong password that you don't use elsewhere."
        ><gw-button variant="outline" href="/account/security/password"
          >Change password</gw-button
        ></gw-card
      ><gw-card
        heading="Multi-factor authentication"
        description=${data.mfa_required ? "Your account policy requires multi-factor authentication." : "Add a second layer of security to your account."}
        >${
          data.factors.length
            ? data.factors.map(
                (factor) =>
                  html`<div class="list-item">
                    <div>
                      <strong
                        >${factor.kind === "totp" ? "Authenticator app" : "Email code"}</strong
                      >
                      <p>
                        ${factor.verified ? "Verified" : "Setup incomplete"}${factor.last_used_at ? ` · Last used ${new Date(factor.last_used_at).toLocaleString()}` : ""}
                      </p>
                    </div>
                    <gw-button
                      variant="ghost"
                      size="sm"
                      ?disabled=${this.busy}
                      @click=${async () => {
                        if (
                          await confirmAction(
                            "Remove verification method?",
                            "Your account must continue to meet its MFA policy.",
                            "Remove",
                          )
                        )
                          void this.run(
                            () => authApi.accountMfaDelete(factor.id),
                            () => this.security.refresh().then(() => undefined),
                            "Verification method removed.",
                          );
                      }}
                      >Remove</gw-button
                    >
                  </div>`,
              )
            : html`<p>No verification methods configured.</p>`
        }
        <div class="form-footer">
          ${available.length > 0 ? html`<gw-button href="/account/security/mfa/new">Add method</gw-button>` : nothing}
        </div></gw-card
      ><gw-card heading="Security requirements"
        >${details({ minimum_factors: data.mfa_requirements.minimum_factors, required_factors: data.mfa_requirements.required_factors.join(", ") || "None", email_verified: data.email_verified })}</gw-card
      >`;
  }
}
define("gw-page-security", SecurityPage);
