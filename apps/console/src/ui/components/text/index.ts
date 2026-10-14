import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwText extends GwElement {
  static properties = { variant: { reflect: true } };
  static styles = componentStyles(styles);
  variant = "body";
  render() {
    return html`<slot></slot>`;
  }
}
define("gw-text", GwText);
