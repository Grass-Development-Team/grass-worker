import { define } from "@/ui/components/base";
import { notify } from "@/ui/components/toast";
import { html } from "lit";
import { Page, field, heading, table } from "../base";
export class ComponentsPage extends Page {
  static properties = { ...Page.properties, open: { state: true } };
  open = false;
  render() {
    this.classList.add("page");
    this.style.maxWidth = "1040px";
    return html`${heading("Component library", "Shared controls, states, and interaction patterns.")}<gw-card
        heading="Buttons"
        ><div class="actions">
          ${["primary", "secondary", "outline", "ghost", "danger"].map((variant) => html`<gw-button variant=${variant} @click=${() => notify(`${variant} button`)}>${variant}</gw-button>`)}<gw-button
            disabled
            >Disabled</gw-button
          ><gw-button loading>Loading</gw-button>
        </div></gw-card
      ><gw-card heading="Form controls"
        ><form
          class="form"
          @submit=${(event: Event) => {
            event.preventDefault();
            notify(
              JSON.stringify(
                Object.fromEntries(new FormData(event.currentTarget as HTMLFormElement)),
              ),
            );
          }}
        >
          <div class="grid">
            ${field({ name: "email", label: "Email", type: "email", required: true })}${field({ name: "password", label: "Password", type: "password", required: true })}${field({ name: "textarea", label: "Textarea", type: "textarea" })}${field(
              {
                name: "select",
                label: "Select",
                options: [
                  { value: "", label: "Choose" },
                  { value: "one", label: "One" },
                  { value: "two", label: "Two" },
                ],
              },
            )}<gw-combobox
              name="search"
              label="Searchable choice"
              .options=${[
                { value: "one", label: "One" },
                { value: "two", label: "Two" },
              ]}
            ></gw-combobox
            ><gw-input-otp name="code"></gw-input-otp
            ><gw-input
              name="invalid"
              label="Error state"
              error="Please check this value."
            ></gw-input
            ><gw-input label="Disabled input" disabled value="Disabled"></gw-input>
          </div>
          <gw-checkbox name="agree" label="Agree to terms"></gw-checkbox
          ><gw-switch name="enabled" label="Enable notifications"></gw-switch
          ><gw-radio-group
            name="choice"
            label="Radio group"
            .options=${[
              { value: "a", label: "First" },
              { value: "b", label: "Second" },
            ]}
          ></gw-radio-group>
          <div class="actions">
            <gw-button type="submit">Submit form</gw-button
            ><gw-button type="reset" variant="outline">Reset</gw-button>
          </div>
        </form></gw-card
      ><gw-card heading="Feedback"
        ><div class="stack">
          ${["neutral", "success", "warning", "error", "info"].map((tone) => html`<gw-alert tone=${tone}>${tone} feedback</gw-alert>`)}
          <div class="actions">
            ${["neutral", "success", "warning", "error", "info"].map((tone) => html`<gw-badge tone=${tone}>${tone}</gw-badge>`)}
          </div>
          <gw-progress value="65" label="Example progress"></gw-progress><gw-skeleton></gw-skeleton
          ><gw-spinner></gw-spinner></div></gw-card
      ><gw-card heading="Layers and navigation"
        ><div class="actions">
          <gw-button @click=${() => (this.open = true)}>Open dialog</gw-button
          ><gw-popover label="Popover"
            ><span slot="trigger">Open popover</span>
            <p>Contextual information.</p></gw-popover
          ><gw-dropdown-menu label="Example menu"
            ><a href="/" role="menuitem">Overview</a
            ><a href="/projects" role="menuitem">Projects</a></gw-dropdown-menu
          ><gw-tooltip text="Helpful information"
            ><gw-button variant="outline">Tooltip</gw-button></gw-tooltip
          >
        </div>
        <gw-tabs
          .tabs=${[
            { value: "first", label: "First tab" },
            { value: "second", label: "Second tab" },
          ]}
          ><p slot="first">First panel.</p>
          <p slot="second">Second panel.</p></gw-tabs
        ></gw-card
      ><gw-dialog
        .open=${this.open}
        heading="Example dialog"
        description="Escape closes this dialog and restores focus."
        @gw-close=${() => (this.open = false)}
        ><p>Dialog content.</p>
        <gw-button slot="footer" @click=${() => (this.open = false)}>Done</gw-button></gw-dialog
      ><gw-card heading="Data display"
        >${table(
          [{ name: "Example", status: "Ready" }],
          [
            { label: "Name", key: "name" },
            { label: "Status", key: "status" },
          ],
        )}<gw-pagination page="1" pages="3"></gw-pagination
        ><gw-empty-state heading="No results">Try another search.</gw-empty-state
        ><gw-avatar alt="Grass Worker"></gw-avatar
      ></gw-card>`;
  }
}
define("gw-page-components", ComponentsPage);
