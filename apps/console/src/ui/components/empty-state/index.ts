import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwEmptyState extends GwElement {
  static properties = { heading: { reflect: true } };
  static styles = componentStyles(styles);
  heading = "Nothing here yet";
  render() {
    return html`<section>
      <gw-icon name="folder"></gw-icon>
      <h2>${this.heading}</h2>
      <slot></slot>
    </section>`;
  }
}
define("gw-empty-state", GwEmptyState);
