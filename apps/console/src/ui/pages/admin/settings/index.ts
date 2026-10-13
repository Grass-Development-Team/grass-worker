import { QueryController } from "@/infra/query";
import { adminApi, type AdminMfaPolicy, type AdminPasswordPolicy } from "@/services/admin";
import { refreshBranding } from "@/services/branding/state";
import { define } from "@/ui/components/base";
import { html, nothing } from "lit";
import { Page, details, field, formValues, heading, readForm, secret, text } from "../../base";
import { settingsFields, settingsValues } from "./fields";
export class SettingsPage extends Page {
  private settings = new QueryController(this, {
    queryKey: ["admin", "settings"],
    queryFn: adminApi.getSettings,
  });
  render() {
    this.classList.add("page");
    const state = this.queryState(this.settings);
    if (state) return state;
    const data = this.settings.result.data!,
      section = this.context.params.section || "basic",
      specs = settingsFields(section),
      values = settingsValues(data);
    if (section === "infrastructure")
      return html`${heading("Infrastructure settings", "Manage regions, storage, certificate issuance, and configuration readiness.")}
        <div class="grid">
          <gw-card
            heading="Regions"
            description="Locations available for nodes, domains, and deployments."
            ><gw-button variant="outline" href="/admin/settings/infrastructure/regions"
              >Manage regions</gw-button
            ></gw-card
          ><gw-card
            heading="HTTPS"
            description="Certificate authority and automatic certificate issuance."
            ><gw-button variant="outline" href="/admin/settings/infrastructure/https"
              >Manage HTTPS</gw-button
            ></gw-card
          ><gw-card heading="Storage" description="Object storage and data migration."
            ><gw-button variant="outline" href="/admin/settings/infrastructure/storage"
              >Manage storage</gw-button
            ></gw-card
          ><gw-card heading="Configuration readiness"
            >${details({ database_configured: data.database.url_configured, redis_backend: data.redis.backend, redis_configured: data.redis.url_configured, control_api_secret_configured: data.secrets.secret_key_configured, git_encryption_configured: data.secrets.git_credentials_configured })}</gw-card
          >
        </div>`;
    if (section === "authentication") {
      const policy = data.authentication.password_policy,
        mfa = data.authentication.mfa_policy;
      return html`${heading("Authentication", "Password requirements, registration, and MFA policy.")}${this.error()}<gw-card
          heading="Password and registration"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  adminApi.updateSettings({
                    registration_email_verification: form.has("registration_email_verification"),
                    password_policy: {
                      min_length: Number(text(form, "min_length")),
                      max_length: Number(text(form, "max_length")),
                      history_count: Number(text(form, "history_count")),
                      require_lowercase: form.has("require_lowercase"),
                      require_uppercase: form.has("require_uppercase"),
                      require_number: form.has("require_number"),
                      require_symbol: form.has("require_symbol"),
                    } satisfies AdminPasswordPolicy,
                  }),
                () => this.settings.refresh().then(() => undefined),
              );
            }}
          >
            <div class="grid">
              ${["min_length", "max_length", "history_count"].map((name) => field({ name, label: name.replaceAll("_", " "), type: "number", min: name === "history_count" ? "0" : "1", required: true }, policy[name as keyof AdminPasswordPolicy]))}
            </div>
            ${["require_lowercase", "require_uppercase", "require_number", "require_symbol"].map((name) => field({ name, label: name.replaceAll("_", " "), type: "boolean" }, policy[name as keyof AdminPasswordPolicy]))}${field({ name: "registration_email_verification", label: "Require email verification on signup", type: "boolean" }, data.authentication.registration_email_verification)}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save password policy</gw-button>
            </div>
          </form></gw-card
        ><gw-card heading="Multi-factor authentication"
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                async () => {
                  const allowed = [
                      ...(form.has("allow_totp") ? ["totp" as const] : []),
                      ...(form.has("allow_email") ? ["email" as const] : []),
                    ],
                    required = [
                      ...(form.has("require_totp") ? ["totp" as const] : []),
                      ...(form.has("require_email") ? ["email" as const] : []),
                    ],
                    minimum = Number(text(form, "minimum_factors"));
                  if (required.some((kind) => !allowed.includes(kind)) || minimum > allowed.length)
                    throw new Error(
                      "Required methods and minimum factors must fit the allowed methods.",
                    );
                  return adminApi.updateSettings({
                    mfa_policy: {
                      allowed_factors: allowed,
                      required_factors: required,
                      minimum_factors: minimum,
                      enforcement: text(form, "enforcement") as AdminMfaPolicy["enforcement"],
                    },
                  });
                },
                () => this.settings.refresh().then(() => undefined),
              );
            }}
          >
            ${field({ name: "allow_totp", label: "Allow authenticator apps", type: "boolean" }, mfa.allowed_factors.includes("totp"))}${field({ name: "allow_email", label: "Allow email codes", type: "boolean" }, mfa.allowed_factors.includes("email"))}${field(
              {
                name: "enforcement",
                label: "MFA enforcement",
                options: [
                  { value: "none", label: "Optional" },
                  { value: "platform_admins", label: "Platform administrators" },
                  { value: "all_users", label: "All users" },
                ],
              },
              mfa.enforcement,
            )}${field({ name: "minimum_factors", label: "Minimum factors", type: "number", min: "0", max: "2", required: true }, mfa.minimum_factors)}${field({ name: "require_totp", label: "Require authenticator app", type: "boolean" }, mfa.required_factors.includes("totp"))}${field({ name: "require_email", label: "Require email code", type: "boolean" }, mfa.required_factors.includes("email"))}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Save MFA policy</gw-button>
            </div>
          </form></gw-card
        >`;
    }
    return html`${heading(`${section[0].toUpperCase() + section.slice(1)} settings`, section === "runtime" ? "These settings require a Control API restart after saving." : "Configure platform defaults.")}${this.error()}${section === "runtime" ? html`<gw-alert tone="info">A Control API restart is required for changes to take effect.</gw-alert>` : nothing}<gw-card
        ><form
          class="form"
          @submit=${(event: Event) => {
            const form = readForm(event);
            void this.run(
              () => {
                const input = formValues(form, specs);
                if (!secret(form, "mail_smtp_password")) delete input.mail_smtp_password;
                return adminApi.updateSettings(input);
              },
              async () => {
                await this.settings.refresh();
                if (section === "basic") await refreshBranding();
              },
            );
          }}
        >
          <div class="grid">${specs.map((spec) => field(spec, values[spec.name]))}</div>
          <div class="form-footer">
            <gw-button type="submit" .loading=${this.busy}>Save settings</gw-button>
          </div>
        </form></gw-card
      >`;
  }
}
define("gw-page-admin-settings", SettingsPage);
