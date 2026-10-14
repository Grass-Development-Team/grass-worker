import { html, unsafeCSS } from "lit";
import { GwElement, define, sharedStyles } from "../base";
import styles from "./style.css?inline";
export class GwProgress extends GwElement {
  static properties = { value: { type: Number }, label: {} };
  static styles = [sharedStyles, unsafeCSS(styles)];
  value = 0;
  label = "Progress";
  render() {
    const value = Math.max(0, Math.min(100, this.value));
    return html`<div
      class="track"
      role="progressbar"
      aria-label=${this.label}
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow=${value}
    >
      <div class="bar" style=${`width:${value}%`}></div>
    </div>`;
  }
}
define("gw-progress", GwProgress);
