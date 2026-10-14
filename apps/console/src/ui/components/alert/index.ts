import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwAlert extends GwElement {
  static properties = { tone: { reflect: true }, heading: {} };
  static styles = componentStyles(styles);
  tone = "info";
  heading = "";
  render() {
    return html`<div role=${this.tone === "error" ? "alert" : "status"}>
      ${this.heading ? html`<strong>${this.heading}</strong>` : ""}<slot></slot>
    </div>`;
  }
}
define("gw-alert", GwAlert);
