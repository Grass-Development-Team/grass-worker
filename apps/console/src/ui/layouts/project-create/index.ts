import { define } from "@/ui/components/base";
import { html, unsafeCSS } from "lit";
import { ConsoleLayout } from "../console";
import styles from "./style.css?inline";

export class ProjectCreateLayout extends ConsoleLayout {
  static styles = [...ConsoleLayout.styles, unsafeCSS(styles)];

  render() {
    return html`<header class="create-header">
        <gw-button variant="ghost" href="/projects"><gw-icon name="back"></gw-icon>Back</gw-button>
        <span class="create-title">New project</span>
        <div class="create-actions">${this.headerActions()}${this.accountMenu(true)}</div>
      </header>
      <main><slot></slot></main>`;
  }
}
define("gw-project-create-layout", ProjectCreateLayout);
