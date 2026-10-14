import { html, svg } from "lit";
import { paths } from "../../../../assets/icons/paths";
import { GwElement, componentStyles, define } from "../base";
import styles from "./style.css?inline";
export class GwIcon extends GwElement {
  static properties = { name: {}, label: {} };
  static styles = componentStyles(styles);
  name = "check";
  label = "";
  render() {
    return html`<svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.7"
      stroke-linecap="round"
      stroke-linejoin="round"
      role=${this.label ? "img" : "presentation"}
      aria-hidden=${String(!this.label)}
      aria-label=${this.label}
    >
      ${(Object.hasOwn(paths, this.name) ? paths[this.name] : paths.check).map((path) => svg`<path d=${path}></path>`)}
    </svg>`;
  }
}
define("gw-icon", GwIcon);
