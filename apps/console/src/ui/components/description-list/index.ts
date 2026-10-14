import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwDescriptionList extends GwElement {
  static properties = { label: { reflect: true } };
  static styles = componentStyles(styles);
  label = "Details";
  render() {
    return html`<dl aria-label=${this.label}><slot></slot></dl>`;
  }
}
define("gw-description-list", GwDescriptionList);
