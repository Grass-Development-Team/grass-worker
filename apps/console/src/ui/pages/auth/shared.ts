import { apiUrl } from "@/infra/api";
import { StoreController } from "@/infra/controllers/store";
import { QueryController } from "@/infra/query";
import { authApi, isAuthResponse, type LoginResponse } from "@/services/auth";
import {
  authHref,
  previewAuthorizationContinuation,
  safeLocalReturnTo,
} from "@/services/auth/continuation";
import { commitSession } from "@/services/auth/state";
import { branding } from "@/services/branding/state";
import { html, nothing } from "lit";
import { Page, field, readForm, secret, text } from "../base";
export class AuthPage extends Page {
  static properties = { ...Page.properties, sent: { state: true } };
  kind = "login";
  sent = false;
  protected brand = new StoreController(this, branding);
  protected configuration = new QueryController(this, {
    queryKey: ["auth-configuration"],
    queryFn: authApi.configuration,
    retry: false,
  });
  protected get returnTo(): string | null {
    return (
      previewAuthorizationContinuation(window.location.search) ??
      safeLocalReturnTo(this.context.search.get("return_to"))
    );
  }
  protected complete(result: LoginResponse): void {
    if (!this.isConnected) return;
    if (!isAuthResponse(result)) {
      this.go(`/mfa#challenge=${encodeURIComponent(result.challenge_token)}`, true);
      return;
    }
    commitSession(result.user);
    const destination = this.returnTo ?? "/";
    if (destination.startsWith("/api/v1/preview/authorize?")) window.location.assign(destination);
    else this.go(destination, true);
  }
  protected brandHeader(title: string, description: unknown = nothing) {
    const brand = this.brand.value;
    return html`<header class="auth-heading">
      <div class="auth-logo">
        ${brand.logo_url ? html`<gw-image src=${brand.logo_url} alt=${`${brand.site_name} logo`}><gw-icon slot="fallback" name="activity"></gw-icon></gw-image>` : html`<gw-icon name="activity" label=${`${brand.site_name} logo`}></gw-icon>`}
      </div>
      <h1>${title}</h1>
      ${description}
    </header>`;
  }
  protected providers() {
    const configuration = this.configuration.result.data;
    return configuration?.providers.length
      ? html`<div class="auth-providers">
          <div class="or"><span></span>or<span></span></div>
          ${configuration.providers.map(
            (provider) =>
              html`<gw-button
                variant="outline"
                @click=${() => {
                  const params = new URLSearchParams();
                  if (this.returnTo) params.set("return_to", this.returnTo);
                  const code = this.querySelector<HTMLInputElement>(
                    "gw-input[name='registration_code']",
                  );
                  if (code?.value) params.set("registration_code", code.value.trim());
                  window.location.assign(
                    apiUrl(
                      `/api/v1/auth/providers/${encodeURIComponent(provider.slug)}/start${params.size ? `?${params}` : ""}`,
                    ),
                  );
                }}
                >Continue with ${provider.name}</gw-button
              >`,
          )}
        </div>`
      : nothing;
  }
  protected submit(event: Event): void {
    const data = readForm(event);
    void this.run(
      async () => {
        const password = secret(data, "password");
        if (this.kind === "signup" || this.kind === "reset") {
          if (password !== secret(data, "confirm")) throw new Error("Passwords do not match.");
        }
        if (this.kind === "login")
          this.complete(await authApi.login(text(data, "email"), password, this.returnTo ?? "/"));
        if (this.kind === "signup") {
          const result = await authApi.register({
            email: text(data, "email"),
            display_name: text(data, "display_name"),
            password,
            return_to: this.returnTo ?? undefined,
            registration_code: text(data, "registration_code") || undefined,
          });
          if (isAuthResponse(result)) this.complete(result);
          else
            this.go(
              `/verify-email?${new URLSearchParams({ email: result.email, ...(this.returnTo ? { return_to: this.returnTo } : {}) })}`,
              true,
            );
        }
        if (this.kind === "forgot") {
          await authApi.forgotPassword(text(data, "email"));
          this.sent = true;
        }
        if (this.kind === "reset") {
          const token = this.context.search.get("token");
          if (!token) throw new Error("The reset link is missing its token.");
          await authApi.resetPassword(token, password);
          this.go("/login", true);
        }
        if (this.kind === "verify") {
          await authApi.resendVerification(text(data, "email"), this.returnTo ?? undefined);
          this.sent = true;
        }
      },
      undefined,
      "",
    );
  }
  protected firstUpdated(): void {
    if (this.kind === "verify" && this.context.search.get("token"))
      void this.run(
        () => authApi.verifyEmail(this.context.search.get("token")!),
        (result) => this.complete(result),
        "",
      );
  }
  render() {
    this.classList.add("auth-page");
    const configuration = this.configuration.result.data;
    const policy = configuration?.password_policy;
    const titles: Record<string, string> = {
      login: `Welcome to ${this.brand.value.site_name}`,
      signup: "Create your account",
      forgot: "Reset your password",
      reset: "Choose a new password",
      verify: "Verify your email",
    };
    const signup = this.kind === "signup",
      login = this.kind === "login",
      passwords = login || signup || this.kind === "reset";
    return html`${this.brandHeader(titles[this.kind], html`<p>${login ? html`New here? <a href=${authHref("/signup", this.returnTo)}>Create an account</a>` : signup ? html`Already have an account? <a href=${authHref("/login", this.returnTo)}>Sign in</a>` : html`<a href=${authHref("/login", this.returnTo)}>Back to sign in</a>`}</p>`)}${this.error()}${this.configuration.result.isError ? html`<gw-alert tone="warning">Sign-in options could not be loaded. <gw-button variant="ghost" @click=${() => this.configuration.refresh()}>Retry</gw-button></gw-alert>` : nothing}${
      signup && configuration?.signup_policy === "closed"
        ? html`<gw-alert tone="warning">Registration is currently closed.</gw-alert>`
        : html`<form class="auth-form" @submit=${this.submit}>
            ${signup ? field({ name: "display_name", label: "Name", required: true, autocomplete: "name" }) : nothing}${this.kind !== "reset" ? html`<gw-input name="email" type="email" label="Email" placeholder="you@example.com" autocomplete="username" required size="lg" .value=${this.context.search.get("email") || ""}><gw-icon slot="prefix" name="user"></gw-icon></gw-input>` : nothing}${passwords ? html`<gw-password-input name="password" label=${login ? "Password" : "New password"} autocomplete=${login ? "current-password" : "new-password"} required size="lg" .minLength=${login ? -1 : (policy?.min_length ?? 8)} .maxLength=${policy?.max_length ?? 1024}><gw-icon slot="prefix" name="key"></gw-icon></gw-password-input>` : nothing}${signup || this.kind === "reset" ? field({ name: "confirm", label: "Confirm password", type: "password", required: true }) : nothing}${signup && configuration?.signup_policy === "invite_only" ? field({ name: "registration_code", label: "Registration code", required: true }) : nothing}${login && configuration?.password_recovery_available ? html`<a class="forgot-link" href="/forgot-password">Forgot password?</a>` : nothing}${this.sent ? html`<gw-alert tone="success">${this.kind === "verify" ? "Verification email sent." : "If an account exists, a recovery email is on its way."}</gw-alert>` : nothing}
            <div class="auth-submit">
              <gw-button type="submit" size="lg" .loading=${this.busy}
                >${login ? "Login" : signup ? "Create account" : this.kind === "reset" ? "Save password" : this.kind === "verify" ? "Resend verification" : "Send recovery email"}</gw-button
              >
            </div>
          </form>`
    }${login || (signup && configuration?.signup_policy !== "closed") ? this.providers() : nothing}`;
  }
}
