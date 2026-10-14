import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwSidebar extends GwElement {
  static properties = { label: { reflect: true } };
  static styles = componentStyles(styles);
  label = "Navigation";
  render() {
    return html`<nav aria-label=${this.label}><slot></slot></nav>`;
  }
}
define("gw-sidebar", GwSidebar);
