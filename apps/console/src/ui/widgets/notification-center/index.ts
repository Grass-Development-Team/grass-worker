import { QueryController, queryClient } from "@/infra/query";
import { errorMessage } from "@/infra/serializer";
import { session } from "@/services/auth/state";
import { notificationsApi, type NotificationItem } from "@/services/notifications";
import { GwElement, define, sharedStyles } from "@/ui/components/base";
import { notify } from "@/ui/components/toast";
import { html, nothing } from "lit";
export class NotificationCenter extends GwElement {
  static properties = { popup: { state: true } };
  static styles = sharedStyles;
  popup: NotificationItem | null = null;
  private seen = new Set<string>();
  private unread = new QueryController(this, {
    queryKey: ["notifications", session.value.user?.id, "unread"],
    queryFn: notificationsApi.unreadCount,
    refetchInterval: 30000,
  });
  private automatic = new QueryController(this, {
    queryKey: ["notifications", session.value.user?.id, "auto-popup"],
    queryFn: notificationsApi.autoPopup,
    refetchInterval: 30000,
  });
  protected updated() {
    const item = this.automatic.result.data?.notification;
    if (item && !this.seen.has(item.id)) {
      this.seen.add(item.id);
      this.popup = item;
    }
  }
  private async dismiss() {
    const popup = this.popup;
    this.popup = null;
    if (!popup) return;
    try {
      await notificationsApi.markRead(popup.id);
      await queryClient.invalidateQueries({ queryKey: ["notifications"] });
    } catch (cause) {
      notify(errorMessage(cause), "error");
    }
  }
  render() {
    return html`<gw-button variant="ghost" href="/notifications"
        ><gw-icon slot="icon" name="bell"></gw-icon
        ><span class="sr-only">Notifications</span
        >${this.unread.result.data?.count ? html`<gw-badge>${this.unread.result.data.count}</gw-badge>` : nothing}</gw-button
      ><gw-dialog
        .open=${Boolean(this.popup)}
        heading=${this.popup?.title || "Announcement"}
        @gw-close=${() => this.dismiss()}
        ><p style="white-space:pre-wrap">${this.popup?.content}</p>
        <gw-button slot="footer" @click=${() => this.dismiss()}>Got it</gw-button></gw-dialog
      >`;
  }
}
define("gw-notification-center", NotificationCenter);
