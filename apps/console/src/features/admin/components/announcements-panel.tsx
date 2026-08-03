import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { SettingsCard } from "@/components/settings-card";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";

import { adminApi } from "../admin.api";

export function AnnouncementsPanel() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["admin", "announcement"],
    queryFn: adminApi.getAnnouncement,
  });
  const [title, setTitle] = useState<string | null>(null);
  const [content, setContent] = useState<string | null>(null);
  const mutation = useMutation({
    mutationFn: adminApi.publishAnnouncement,
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["admin", "announcement"] }),
        queryClient.invalidateQueries({ queryKey: ["notifications"] }),
      ]);
    },
  });

  if (query.isPending) return <Skeleton className="h-72 w-full" aria-busy="true" />;
  if (query.isError) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {query.error.message}
      </p>
    );
  }

  const currentTitle = title ?? query.data.title ?? "";
  const currentContent = content ?? query.data.content ?? "";
  const error =
    mutation.error instanceof Error ? mutation.error.message : "Unable to publish announcement.";

  return (
    <SettingsCard
      title="Website announcement"
      description="Publish a plain-text announcement to every active account. Each recipient can open it from their notifications."
      hint="Publishing replaces the current website announcement and creates a new unread notification."
      action={
        <Button type="submit" form="announcement-form" disabled={mutation.isPending}>
          {mutation.isPending ? "Publishing..." : "Publish"}
        </Button>
      }
    >
      <form
        id="announcement-form"
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          mutation.mutate({ title: currentTitle, content: currentContent });
        }}
      >
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="announcement-title">Title</FieldLabel>
            <Input
              id="announcement-title"
              maxLength={120}
              value={currentTitle}
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
              rows={10}
              value={currentContent}
              onChange={(event) => setContent(event.target.value)}
              required
            />
            <FieldDescription>Plain text only, up to 10,000 characters.</FieldDescription>
          </Field>
        </FieldGroup>
        {mutation.isSuccess && (
          <p className="text-sm text-muted-foreground">Announcement published.</p>
        )}
        {mutation.isError && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
      </form>
    </SettingsCard>
  );
}
