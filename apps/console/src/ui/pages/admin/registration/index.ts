import { QueryController } from "@/infra/query";
import { adminApi } from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html } from "lit";
import { Page, field, heading, readForm, table, text } from "../../base";
export class RegistrationPage extends Page {
  private emails = new QueryController(this, {
    queryKey: ["admin", "registration"],
    queryFn: adminApi.listRegistrationEmails,
  });
  render() {
    this.classList.add("page");
    return html`${heading("Registration allowlist", "Email addresses permitted by invitation-only registration.")}${this.error()}<gw-card
        heading="Allow an email address"
        ><form
          class="form"
          @submit=${(event: Event) => {
            const form = readForm(event);
            void this.run(
              () => adminApi.addRegistrationEmail({ email: text(form, "email") }),
              () => this.emails.refresh().then(() => undefined),
              "Email allowed.",
            );
          }}
        >
          ${field({ name: "email", label: "Email", type: "email", required: true })}
          <div class="form-footer">
            <gw-button type="submit" .loading=${this.busy}>Add email</gw-button>
          </div>
        </form></gw-card
      >${
        this.queryState(this.emails) ??
        table(
          this.emails.result.data!.emails,
          [
            { label: "Email", key: "email" },
            { label: "Added", key: "created_at" },
            {
              label: "Actions",
              render: (entry) =>
                html`<gw-button
                  variant="ghost"
                  size="sm"
                  @click=${async () => {
                    if (
                      await confirmAction(
                        "Remove email?",
                        `${entry.email} will no longer be allowlisted.`,
                        "Remove",
                      )
                    )
                      void this.run(
                        () => adminApi.removeRegistrationEmail(entry.id),
                        () => this.emails.refresh().then(() => undefined),
                        "Email removed.",
                      );
                  }}
                  >Remove</gw-button
                >`,
            },
          ],
          "Registration allowlist",
        )
      }`;
  }
}
define("gw-page-admin-registration", RegistrationPage);
