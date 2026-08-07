import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeftIcon, ArrowRightIcon, MegaphoneIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";

import { SettingsCard } from "@/components/settings-card";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import type { Announcement } from "@/features/announcements/announcements.api";

import { adminApi } from "../admin.api";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

export function AnnouncementsPanel() {
  const queryClient = useQueryClient();
  const [page, setPage] = useState(1);
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [autoPopup, setAutoPopup] = useState(false);
  const [deleting, setDeleting] = useState<Announcement | null>(null);
  const listQuery = useQuery({
    queryKey: ["admin", "announcements", page],
    queryFn: () => adminApi.listAnnouncements(page),
  });
  const publishMutation = useMutation({
    mutationFn: adminApi.publishAnnouncement,
    onSuccess: async () => {
      setTitle("");
      setContent("");
      setAutoPopup(false);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["admin", "announcements"] }),
        queryClient.invalidateQueries({ queryKey: ["announcements"] }),
        queryClient.invalidateQueries({ queryKey: ["notifications"] }),
      ]);
    },
  });
  const deleteMutation = useMutation({
    mutationFn: adminApi.removeAnnouncement,
    onSuccess: async () => {
      setDeleting(null);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["admin", "announcements"] }),
        queryClient.invalidateQueries({ queryKey: ["announcements"] }),
        queryClient.invalidateQueries({ queryKey: ["notifications"] }),
      ]);
    },
  });

  if (listQuery.isPending) return <Skeleton className="h-[38rem] w-full" aria-busy="true" />;
  if (listQuery.isError) return null;
  const data = listQuery.data;

  return (
    <div className="flex flex-col gap-6">
      <SettingsCard
        title="Publish announcement"
        description="Create a plain-text announcement for every active account."
        hint="Each publish creates a new history item and notification."
        action={
          <Button
            type="submit"
            form="announcement-form"
            disabled={publishMutation.isPending || !title.trim() || !content.trim()}
          >
            {publishMutation.isPending ? "Publishing..." : "Publish"}
          </Button>
        }
      >
        <form
          id="announcement-form"
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            publishMutation.mutate({
              title,
              content,
              auto_popup: autoPopup,
            });
          }}
        >
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="announcement-title">Title</FieldLabel>
              <Input
                id="announcement-title"
                maxLength={120}
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                required
              />
              <FieldDescription>Up to 120 characters.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="announcement-content">Content</FieldLabel>
              <Textarea
                id="announcement-content"
                maxLength={10_000}
                rows={8}
                value={content}
                onChange={(event) => setContent(event.target.value)}
                required
              />
              <FieldDescription>Plain text only, up to 10,000 characters.</FieldDescription>
            </Field>
            <label className="flex items-center justify-between gap-4 rounded-md border px-3 py-3">
              <span>
                <span className="block text-sm font-medium">Show as a popup</span>
                <span className="block text-xs text-muted-foreground">
                  The newest unread popup announcement opens when the user enters the Console.
                </span>
              </span>
              <Switch
                checked={autoPopup}
                onCheckedChange={setAutoPopup}
                aria-label="Show as a popup"
              />
            </label>
          </FieldGroup>
          {publishMutation.isSuccess && (
            <p className="text-sm text-muted-foreground">Announcement published.</p>
          )}
        </form>
      </SettingsCard>

      <section className="flex flex-col gap-3" aria-labelledby="announcement-history-heading">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <h2 id="announcement-history-heading" className="text-lg font-semibold">
              Announcement history
            </h2>
            <p className="text-sm text-muted-foreground">{data.pagination.total} total</p>
          </div>
        </div>
        {data.announcements.length === 0 ? (
          <div className="flex min-h-32 items-center justify-center border-y text-sm text-muted-foreground">
            No announcements yet.
          </div>
        ) : (
          <div className="divide-y overflow-hidden rounded-md border">
            {data.announcements.map((announcement) => (
              <article key={announcement.id} className="flex gap-4 px-4 py-4">
                <span className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
                  <MegaphoneIcon className="size-4" />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                    <h3 className="font-medium">{announcement.title}</h3>
                    {announcement.auto_popup && (
                      <span className="text-xs text-muted-foreground">Popup</span>
                    )}
                  </div>
                  <time
                    dateTime={announcement.published_at}
                    className="text-xs text-muted-foreground"
                  >
                    {formatTimestamp(announcement.published_at)}
                  </time>
                  <p className="mt-2 whitespace-pre-wrap break-words text-sm text-muted-foreground">
                    {announcement.content}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="shrink-0"
                  aria-label={`Delete ${announcement.title}`}
                  onClick={() => setDeleting(announcement)}
                >
                  <Trash2Icon />
                </Button>
              </article>
            ))}
          </div>
        )}
        {data.pagination.total_pages > 1 && (
          <nav className="flex items-center justify-between" aria-label="Announcement pages">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setPage((current) => Math.max(1, current - 1))}
              disabled={page <= 1}
              aria-label="Previous page"
            >
              <ArrowLeftIcon />
            </Button>
            <span className="text-xs text-muted-foreground">
              {data.pagination.page} / {data.pagination.total_pages}
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setPage((current) => current + 1)}
              disabled={page >= data.pagination.total_pages}
              aria-label="Next page"
            >
              <ArrowRightIcon />
            </Button>
          </nav>
        )}
      </section>

      <Dialog open={Boolean(deleting)} onOpenChange={(open) => !open && setDeleting(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Delete announcement</DialogTitle>
            <DialogDescription>
              This removes the announcement from history and deletes its user notifications.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleting(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => deleting && deleteMutation.mutate(deleting.id)}
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending ? "Deleting..." : "Delete announcement"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
