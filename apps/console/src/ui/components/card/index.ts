import { html, nothing } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwCard extends GwElement {
  static properties = { heading: {}, description: {} };
  static styles = componentStyles(styles);
  heading = "";
  description = "";
  render() {
    return html`<section>
      ${this.heading || this.description ? html`<header>${this.heading ? html`<h2>${this.heading}</h2>` : nothing}${this.description ? html`<p>${this.description}</p>` : nothing}<slot name="action"></slot></header>` : nothing}
      <div class="content"><slot></slot></div>
      <footer><slot name="footer"></slot></footer>
    </section>`;
  }
}
define("gw-card", GwCard);
