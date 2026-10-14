import { html } from "lit";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwBadge extends GwElement {
  static properties = { tone: { reflect: true } };
  static styles = componentStyles(styles);
  tone = "neutral";
  render() {
    return html`<span><slot></slot></span>`;
  }
}
define("gw-badge", GwBadge);
