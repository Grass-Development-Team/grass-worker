import { isBuildRunning, type BuildStatus } from "@/services/deployments";
import { BuildLogStream, type LogState } from "@/services/deployments/log-stream";
import { GwElement, componentStyles, define } from "@/ui/components/base";
import { html, type PropertyValues } from "lit";
import styles from "./style.css?inline";
export class DeploymentLog extends GwElement {
  static properties = {
    projectId: {},
    deploymentId: {},
    status: {},
    follow: { state: true },
    state: { state: true },
  };
  static styles = componentStyles(styles);
  projectId = "";
  deploymentId = "";
  status: BuildStatus = "pending";
  follow = true;
  state: LogState = { lines: [], connected: false, error: "" };
  private stream?: BuildLogStream;
  private unsubscribe?: () => void;
  protected updated(changed: PropertyValues): void {
    if (changed.has("projectId") || changed.has("deploymentId")) {
      this.stream?.stop();
      this.unsubscribe?.();
      if (this.projectId && this.deploymentId) {
        this.stream = new BuildLogStream(this.projectId, this.deploymentId, () =>
          this.dispatchEvent(new CustomEvent("gw-done", { bubbles: true, composed: true })),
        );
        this.state = this.stream.state.value;
        this.unsubscribe = this.stream.state.subscribe(() => {
          this.state = this.stream!.state.value;
        });
        this.stream.start(isBuildRunning(this.status));
      }
    } else if (changed.has("status")) this.stream?.setRunning(isBuildRunning(this.status));
    if (this.follow) {
      const output = this.renderRoot.querySelector<HTMLElement>(".output");
      if (output) output.scrollTop = output.scrollHeight;
    }
  }
  disconnectedCallback() {
    this.stream?.stop();
    this.unsubscribe?.();
    super.disconnectedCallback();
  }
  render() {
    return html`<div class="toolbar">
        <span
          ><gw-badge
            tone=${this.state.connected ? "success" : isBuildRunning(this.status) ? "warning" : "neutral"}
            >${this.state.connected ? "Live" : isBuildRunning(this.status) ? "Reconnecting" : "Build finished"}</gw-badge
          >
          ${this.state.lines.length} lines</span
        ><gw-button variant="ghost" size="sm" @click=${() => (this.follow = !this.follow)}
          >${this.follow ? "Pause scroll" : "Follow"}</gw-button
        >
      </div>
      ${this.state.error ? html`<p class="error" role="status">${this.state.error}</p>` : ""}
      <div
        class="output"
        role="log"
        aria-label="Build log"
        aria-live="off"
        tabindex="0"
        @scroll=${(event: Event) => {
          const output = event.currentTarget as HTMLElement;
          if (output.scrollHeight - output.scrollTop - output.clientHeight > 24)
            this.follow = false;
        }}
      >
        ${this.state.lines.length ? this.state.lines.map((line) => html`<div><span class="stage">[${line.stage}]</span><span>${line.line}</span></div>`) : html`<p>${isBuildRunning(this.status) ? "Waiting for build output…" : "No log output recorded."}</p>`}
      </div>`;
  }
}
define("gw-deployment-log", DeploymentLog);
