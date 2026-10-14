import { QueryController, queryClient } from "@/infra/query";
import { safeLocalReturnTo } from "@/services/auth/continuation";
import { session } from "@/services/auth/state";
import { notificationsApi } from "@/services/notifications";
import { define } from "@/ui/components/base";
import { html } from "lit";
import { Page, badge, heading } from "../base";
export class NotificationsPage extends Page {
  static properties = { ...Page.properties, page: { state: true } };
  page = 1;
  private notifications = new QueryController(this, {
    queryKey: ["notifications", session.value.user?.id, 1],
    queryFn: () => notificationsApi.list(this.page),
    refetchInterval: 30000,
  });
  private next(page: number) {
    this.page = page;
    this.notifications.configure({
      queryKey: ["notifications", session.value.user?.id, page],
      queryFn: () => notificationsApi.list(page),
      refetchInterval: 30000,
    });
  }
  private refresh() {
    return queryClient.invalidateQueries({ queryKey: ["notifications"] });
  }
  render() {
    this.classList.add("page");
    const state = this.queryState(this.notifications);
    const data = this.notifications.result.data;
    return html`${heading("Notifications", "Deployment updates, team activity, and announcements.", html`<gw-button variant="outline" .loading=${this.busy} @click=${() => this.run(notificationsApi.markAllRead, () => this.refresh(), "All notifications marked as read.")}>Mark all as read</gw-button>`)}${
      state ??
      html`<gw-card
          >${
            data!.notifications.length
              ? data!.notifications.map(
                  (item) =>
                    html`<article class="list-item">
                      <div>
                        <div class="row">
                          <h3 style="margin-bottom:0">${item.title}</h3>
                          ${!item.read_at ? badge("New") : ""}
                        </div>
                        ${item.project ? html`<p>${item.project.name}</p>` : ""}${item.content ? html`<p style="white-space:pre-wrap">${item.content}</p>` : ""}${item.reason ? html`<p>${item.reason}</p>` : ""}
                        <p>${new Date(item.created_at).toLocaleString()}</p>
                      </div>
                      <div class="actions">
                        ${
                          safeLocalReturnTo(item.target_url)
                            ? html`<gw-button
                                variant="outline"
                                size="sm"
                                href=${item.target_url}
                                @click=${() =>
                                  this.run(
                                    () => notificationsApi.markRead(item.id),
                                    () => this.refresh(),
                                    "",
                                  )}
                                >Open</gw-button
                              >`
                            : ""
                        }${
                          !item.read_at
                            ? html`<gw-button
                                variant="ghost"
                                size="sm"
                                @click=${() =>
                                  this.run(
                                    () => notificationsApi.markRead(item.id),
                                    () => this.refresh(),
                                    "",
                                  )}
                                >Mark read</gw-button
                              >`
                            : ""
                        }
                      </div>
                    </article>`,
                )
              : html`<gw-empty-state heading="You're all caught up"></gw-empty-state>`
          }</gw-card
        ><gw-pagination
          .page=${this.page}
          .pages=${data!.pagination.total_pages}
          @gw-page=${(event: CustomEvent<{ page: number }>) => this.next(event.detail.page)}
        ></gw-pagination>`
    }${this.error()}`;
  }
}
define("gw-page-notifications", NotificationsPage);
