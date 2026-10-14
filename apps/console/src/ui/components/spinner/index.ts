import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwSpinner extends GwElement {
  static properties = { label: { reflect: true } };
  static styles = componentStyles(styles);
  label = "Loading";
  render() {
    return html`<span role="status" aria-label=${this.label}></span>`;
  }
}
define("gw-spinner", GwSpinner);
