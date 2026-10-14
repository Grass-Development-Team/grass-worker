import { QueryController, queryClient } from "@/infra/query";
import {
  authApi,
  isAuthResponse,
  type MfaChallenge,
  type MfaFactor,
  type TotpEnrollment,
} from "@/services/auth";
import { previewAuthorizationContinuation, safeLocalReturnTo } from "@/services/auth/continuation";
import { commitSession } from "@/services/auth/state";
import { define } from "@/ui/components/base";
import { html, nothing } from "lit";
import QRCode from "qrcode";
import { AuthPage } from "../auth/shared";
import { readForm, text } from "../base";
export class MfaPage extends AuthPage {
  static properties = {
    ...AuthPage.properties,
    challenge: { state: true },
    factor: { state: true },
    enrollment: { state: true },
    qr: { state: true },
    emailCooldown: { state: true },
  };
  private token = new URLSearchParams(window.location.hash.slice(1)).get("challenge") || "";
  private challengeQuery = new QueryController(this, {
    queryKey: ["mfa", this.token],
    queryFn: () => authApi.mfaChallenge(this.token),
    enabled: Boolean(this.token),
    retry: false,
    gcTime: 0,
  });
  challenge: MfaChallenge | null = null;
  factor: MfaFactor | null = null;
  enrollment: TotpEnrollment | null = null;
  qr = "";
  emailCooldown = 0;
  private cooldown?: ReturnType<typeof setInterval>;
  disconnectedCallback(): void {
    clearInterval(this.cooldown);
    this.enrollment = null;
    this.qr = "";
    this.token = "";
    super.disconnectedCallback();
  }
  private choose(kind: "totp" | "email"): void {
    const challenge = this.challenge ?? this.challengeQuery.result.data;
    if (!challenge) return;
    void this.run(
      async () => {
        this.enrollment = null;
        this.qr = "";
        if (kind === "totp") {
          if (challenge.mfa_enrollment_required) {
            const enrollment = await authApi.mfaTotpStart(this.token);
            this.enrollment = enrollment;
            this.factor = enrollment.factor;
            this.qr = await QRCode.toDataURL(enrollment.otpauth_uri, {
              width: 208,
              margin: 2,
              errorCorrectionLevel: "M",
            });
          } else this.factor = challenge.factors.find((factor) => factor.kind === "totp") ?? null;
        } else {
          const selected = challenge.mfa_enrollment_required
            ? undefined
            : challenge.factors.find((factor) => factor.kind === "email");
          this.factor = (await authApi.mfaEmailSend(this.token, selected?.id)).factor;
          this.emailCooldown = 30;
          clearInterval(this.cooldown);
          this.cooldown = setInterval(() => {
            this.emailCooldown--;
            if (this.emailCooldown <= 0) clearInterval(this.cooldown);
          }, 1000);
        }
        await this.updateComplete;
        this.querySelector<HTMLElement>("gw-input-otp")?.focus();
      },
      undefined,
      "",
    );
  }
  private verify(event: Event): void {
    const data = readForm(event);
    if (!this.factor) return;
    void this.run(
      () => authApi.mfaVerify(this.token, this.factor!.id, text(data, "code")),
      async (result) => {
        if (!isAuthResponse(result)) {
          this.challenge = result;
          this.factor = null;
          this.enrollment = null;
          this.qr = "";
          return;
        }
        const challenge = this.challenge ?? this.challengeQuery.result.data;
        const raw = challenge?.return_to;
        const destination = raw?.startsWith("/api/v1/preview/authorize?")
          ? previewAuthorizationContinuation(`?${new URLSearchParams({ continue: raw })}`)
          : safeLocalReturnTo(raw);
        queryClient.removeQueries({ queryKey: ["mfa", this.token] });
        commitSession(result.user);
        if (destination?.startsWith("/api/")) window.location.assign(destination);
        else this.go(destination ?? "/", true);
      },
      "",
    );
  }
  render() {
    this.classList.add("auth-page", "mfa-page");
    if (!this.token)
      return html`${this.brandHeader("Verification link expired")}<gw-alert tone="warning"
          >Start a new sign-in to receive a verification challenge.</gw-alert
        ><gw-button href="/login">Back to sign in</gw-button>`;
    const state = this.queryState(this.challengeQuery);
    if (state) return state;
    const challenge = this.challenge ?? this.challengeQuery.result.data!;
    const enrolling = challenge.mfa_enrollment_required;
    const kinds = enrolling
      ? challenge.allowed_factors.filter(
          (kind) => !challenge.factors.some((factor) => factor.kind === kind && factor.verified),
        )
      : [
          ...new Set(
            challenge.factors.filter((factor) => factor.verified).map((factor) => factor.kind),
          ),
        ];
    return html`${this.brandHeader(enrolling ? "Secure your account" : "Verify your sign-in", html`<p>${this.factor ? (this.factor.kind === "email" ? "Enter the code sent to your verified email." : "Enter the six-digit code from your authenticator app.") : enrolling ? "Choose a verification method. Your account policy may require more than one." : "Choose how you want to verify this sign-in."}</p>`)}${this.error()}${
        !this.factor
          ? html`<div class="mfa-methods">
                ${kinds.map(
                  (kind) =>
                    html`<button
                      class="method"
                      type="button"
                      ?disabled=${this.busy}
                      @click=${() => this.choose(kind)}
                    >
                      <gw-icon name=${kind === "totp" ? "phone" : "mail"}></gw-icon
                      ><span
                        ><strong>${kind === "totp" ? "Authenticator app" : "Email code"}</strong
                        ><small
                          >${kind === "totp" ? "Use a time-based code from your device" : "Receive a code in your inbox"}</small
                        ></span
                      ><gw-icon name="chevron"></gw-icon>
                    </button>`,
                )}
              </div>
              ${!kinds.length ? html`<gw-alert tone="warning">No verification method is available. Contact your administrator.</gw-alert>` : nothing}`
          : html`<div class="stack">
              ${
                this.enrollment
                  ? html`<ol class="enrollment-steps">
                        <li>Add an account in your authenticator app.</li>
                        <li>Scan this QR code or enter the setup key.</li>
                        <li>Enter the current code below to finish.</li>
                      </ol>
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
                          size="sm"
                          @click=${() => this.run(() => navigator.clipboard.writeText(this.enrollment!.secret), undefined, "Setup key copied.")}
                          >Copy key</gw-button
                        >
                      </div>`
                  : nothing
              }
              <form class="form" @submit=${this.verify}>
                <gw-input-otp name="code" required></gw-input-otp
                ><gw-button type="submit" size="lg" .loading=${this.busy}
                  >${enrolling ? "Add verification method" : "Verify and continue"}</gw-button
                >
              </form>
              <div class="row">
                <gw-button
                  variant="ghost"
                  ?disabled=${this.busy}
                  @click=${() => {
                    this.factor = null;
                    this.enrollment = null;
                    this.qr = "";
                    this.failure = "";
                  }}
                  >Use another method</gw-button
                >${this.factor.kind === "email" ? html`<gw-button variant="ghost" ?disabled=${this.emailCooldown > 0 || this.busy} @click=${() => this.choose("email")}>${this.emailCooldown > 0 ? `Resend in ${this.emailCooldown}s` : "Resend code"}</gw-button>` : nothing}
              </div>
            </div>`
      }
      <p class="auth-back"><a href="/login">Back to sign in</a></p>`;
  }
}
define("gw-page-mfa", MfaPage);
