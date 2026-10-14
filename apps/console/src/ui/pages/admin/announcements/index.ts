import { QueryController } from "@/infra/query";
import { adminApi } from "@/services/admin";
import { define } from "@/ui/components/base";
import { confirmAction } from "@/ui/components/dialog/confirm";
import { html } from "lit";
import { Page, field, heading, readForm, table, text } from "../../base";
export class AnnouncementsPage extends Page {
  static properties = { ...Page.properties, page: { state: true } };
  page = 1;
  private announcements = new QueryController(this, {
    queryKey: ["admin", "announcements", 1],
    queryFn: () => adminApi.listAnnouncements(1),
  });
  private next(page: number) {
    this.page = page;
    this.announcements.configure({
      queryKey: ["admin", "announcements", page],
      queryFn: () => adminApi.listAnnouncements(page),
    });
  }
  render() {
    this.classList.add("page");
    if (this.context.path.endsWith("/new"))
      return html`${heading("Publish announcement", "Send an announcement to platform users.", html`<gw-button variant="ghost" href="/admin/settings/announcements">Back to announcements</gw-button>`)}${this.error()}<gw-card
          ><form
            class="form"
            @submit=${(event: Event) => {
              const form = readForm(event);
              void this.run(
                () =>
                  adminApi.publishAnnouncement({
                    title: text(form, "title"),
                    content: text(form, "content"),
                    auto_popup: form.has("auto_popup"),
                  }),
                async () => {
                  await this.announcements.refresh();
                  this.go("/admin/settings/announcements");
                },
                "Announcement published.",
              );
            }}
          >
            ${field({ name: "title", label: "Title", required: true })}${field({ name: "content", label: "Content", type: "textarea", required: true })}${field({ name: "auto_popup", label: "Show automatically to users", type: "boolean" }, false)}
            <div class="form-footer">
              <gw-button type="submit" .loading=${this.busy}>Publish announcement</gw-button>
            </div>
          </form></gw-card
        >`;
    const data = this.announcements.result.data;
    return html`${heading("Announcements", "Publish and manage platform announcements.", html`<gw-button href="/admin/settings/announcements/new">Publish announcement</gw-button>`)}${this.error()}${
      this.queryState(this.announcements) ??
      html`${table(
          data!.announcements,
          [
            { label: "Title", key: "title" },
            { label: "Content", key: "content" },
            { label: "Published", key: "published_at" },
            {
              label: "Actions",
              render: (item) =>
                html`<gw-button
                  variant="ghost"
                  size="sm"
                  @click=${async () => {
                    if (
                      await confirmAction(
                        "Remove announcement?",
                        "This announcement will no longer be visible.",
                        "Remove",
                      )
                    )
                      void this.run(
                        () => adminApi.removeAnnouncement(item.id),
                        () => this.announcements.refresh().then(() => undefined),
                        "Announcement removed.",
                      );
                  }}
                  >Remove</gw-button
                >`,
            },
          ],
          "Announcements",
        )}<gw-pagination
          .page=${this.page}
          .pages=${data!.pagination.total_pages}
          @gw-page=${(event: CustomEvent<{ page: number }>) => this.next(event.detail.page)}
        ></gw-pagination>`
    }`;
  }
}
define("gw-page-admin-announcements", AnnouncementsPage);
