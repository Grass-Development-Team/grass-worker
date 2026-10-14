import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwFieldGroup extends GwElement {
  static properties = { label: { reflect: true } };
  static styles = componentStyles(styles);
  label = "";
  render() {
    return html`<div role="group" aria-label=${this.label}><slot></slot></div>`;
  }
}
define("gw-field-group", GwFieldGroup);
