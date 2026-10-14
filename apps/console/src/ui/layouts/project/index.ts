import { GwElement, componentStyles, define } from "@/ui/components/base";
import { html } from "lit";
import styles from "./style.css?inline";
export class ProjectLayout extends GwElement {
  static styles = componentStyles(styles);
  static properties = { projectId: {}, current: {} };
  projectId = "";
  current = "";
  render() {
    const base = `/projects/${encodeURIComponent(this.projectId)}`;
    const tabs = [
      ["Overview", base],
      ["Deployments", `${base}/deployments`],
      ["Domains", `${base}/domains`],
      ["Settings", `${base}/settings`],
    ];
    return html`<nav aria-label="Project pages">
        ${tabs.map(([label, href]) => html`<a href=${href} aria-current=${this.current === href || (href !== base && this.current.startsWith(href)) ? "page" : "false"}>${label}</a>`)}
      </nav>
      <slot></slot>`;
  }
}
define("gw-project-layout", ProjectLayout);
