import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArchiveIcon, ArchiveRestoreIcon, CheckIcon, CopyIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SettingsCard } from "@/components/settings-card";
import { useBranding } from "@/features/branding/branding-context";
import {
  canContributeToProjects,
  canManageProjectLifecycle,
} from "@/features/teams/team-permissions";

import { projectsApi } from "./projects.api";
import { useProject } from "./project-layout";

export function ProjectSettingsRoute() {
  const { siteName } = useBranding();
  const { project, role } = useProject();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState(project.name);
  const [nameError, setNameError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const canEdit = canContributeToProjects(role);
  const canManageLifecycle = canManageProjectLifecycle(role);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["project", project.id] });

  const nameMutation = useMutation({
    mutationFn: () => projectsApi.update(project.id, { name }),
    onSuccess: () => {
      setNameError(null);
      invalidate();
    },
    onError: (cause) =>
      setNameError(cause instanceof Error ? cause.message : "Unable to save the project name."),
  });

  const archiveMutation = useMutation({
    mutationFn: () =>
      project.archived_at ? projectsApi.unarchive(project.id) : projectsApi.archive(project.id),
    onSuccess: invalidate,
  });

  const deleteMutation = useMutation({
    mutationFn: () => projectsApi.softDelete(project.id),
    onSuccess: () => navigate("/projects"),
  });

  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(project.id);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied; the ID stays selectable in the input.
    }
  };

  return (
    <div className="space-y-6">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (canEdit) nameMutation.mutate();
        }}
      >
        <SettingsCard
          title="Project Name"
          description="Used to identify your project on the Console and in deployment URLs."
          hint={`The URL slug stays ${project.slug}.`}
          action={
            canEdit ? (
              <Button type="submit" size="sm" disabled={nameMutation.isPending || !name.trim()}>
                {nameMutation.isPending ? "Saving…" : "Save"}
              </Button>
            ) : undefined
          }
        >
          <Input
            id="settings-name"
            aria-label="Project name"
            className="max-w-sm"
            value={name}
            readOnly={!canEdit}
            onChange={(event) => setName(event.target.value)}
          />
          {nameError && (
            <p role="alert" className="mt-2 text-sm text-destructive">
              {nameError}
            </p>
          )}
        </SettingsCard>
      </form>

      <SettingsCard
        title="Project ID"
        description={`Used when interacting with the ${siteName} API.`}
        hint="The project ID cannot be changed."
      >
        <div className="flex max-w-md items-center gap-2">
          <Input
            readOnly
            value={project.id}
            className="font-mono text-xs"
            aria-label="Project ID"
          />
          <Button
            type="button"
            variant="outline"
            size="icon"
            onClick={copyId}
            aria-label={copied ? "Project ID copied" : "Copy Project ID"}
          >
            {copied ? <CheckIcon /> : <CopyIcon />}
          </Button>
        </div>
      </SettingsCard>

      <SettingsCard
        title={project.archived_at ? "Unarchive Project" : "Archive Project"}
        description={
          project.archived_at
            ? "Resume deployments for this project."
            : "Pause deployments without deleting anything. Existing deployments keep serving."
        }
        hint="Archiving can be reverted at any time."
        action={
          canManageLifecycle ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => archiveMutation.mutate()}
              disabled={archiveMutation.isPending}
            >
              {project.archived_at ? (
                <>
                  <ArchiveRestoreIcon /> Unarchive
                </>
              ) : (
                <>
                  <ArchiveIcon /> Archive
                </>
              )}
            </Button>
          ) : undefined
        }
      />

      <SettingsCard
        variant="destructive"
        title="Delete Project"
        description="The project is soft-deleted and stops serving immediately. A platform administrator can restore it."
        hint="Please make sure this is what you want."
        action={
          canManageLifecycle ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="destructive" size="sm" disabled={deleteMutation.isPending}>
                  <Trash2Icon /> Delete
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete this project?</AlertDialogTitle>
                  <AlertDialogDescription>
                    The project is soft-deleted and can be restored by an administrator. Active
                    deployments stop being served.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    className={buttonVariants({ variant: "destructive" })}
                    onClick={() => deleteMutation.mutate()}
                  >
                    Delete Project
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          ) : undefined
        }
      />
    </div>
  );
}
