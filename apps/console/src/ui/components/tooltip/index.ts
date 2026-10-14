import { html, unsafeCSS } from "lit";
import { GwElement, define, sharedStyles } from "../base";
import styles from "./style.css?inline";
export class GwTooltip extends GwElement {
  static properties = { text: {} };
  static styles = [sharedStyles, unsafeCSS(styles)];
  text = "";
  render() {
    return html`<slot></slot><span role="tooltip" class="tip">${this.text}</span>`;
  }
}
define("gw-tooltip", GwTooltip);
