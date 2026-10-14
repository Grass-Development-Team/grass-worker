import { define } from "@/ui/components/base";
import { css } from "lit";
import { ConsoleLayout } from "../console";
export class ProjectCreateLayout extends ConsoleLayout {
  static styles = [
    ...ConsoleLayout.styles,
    css`
      aside {
        display: none;
      }
      .main,
      .collapsed .main {
        margin-left: 0;
      }
      header > gw-button:first-child {
        display: none;
      }
      main {
        max-width: 1120px;
      }
    `,
  ];
}
define("gw-project-create-layout", ProjectCreateLayout);
