import { html } from "lit";
import { define } from "../base";
import { GwDialog } from "../dialog";
export class GwCommand extends GwDialog {
  static properties = { ...GwDialog.properties, query: {}, items: { attribute: false } };
  query = "";
  items: { label: string; href: string }[] = [];
  constructor() {
    super();
    this.heading = "Jump to";
  }
  render() {
    return html`<dialog aria-label=${this.heading} @cancel=${() => this.close()}>
      <gw-input
        label="Search pages"
        .value=${this.query}
        @gw-input=${(event: CustomEvent<{ value: string }>) => (this.query = event.detail.value)}
      ></gw-input>
      <nav>
        ${this.items.filter((item) => item.label.toLowerCase().includes(this.query.toLowerCase())).map((item) => html`<p><gw-button variant="ghost" href=${item.href} @click=${() => this.close()}>${item.label}</gw-button></p>`)}
      </nav>
      <gw-button variant="outline" @click=${this.close}>Close</gw-button>
    </dialog>`;
  }
}
define("gw-command", GwCommand);
