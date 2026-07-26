import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArchiveIcon, ArchiveRestoreIcon, FolderGit2Icon, Trash2Icon } from "lucide-react";
import { useState } from "react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

import {
  BuildStatusBadge,
  ReleaseStatusBadge,
} from "@/features/deployments/components/status-badges";

import { adminApi, type AdminProject } from "../admin.api";

export function ProjectsPanel() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<AdminProject | null>(null);

  const projectsQuery = useQuery({
    queryKey: ["admin", "projects", query],
    queryFn: () => adminApi.listProjects(query),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "projects"] });

  const archiveMutation = useMutation({
    mutationFn: (project: AdminProject) =>
      project.archived_at
        ? adminApi.unarchiveProject(project.id)
        : adminApi.archiveProject(project.id),
    onSuccess: () => {
      setError(null);
      invalidate();
    },
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to update the project."),
  });

  const deleteMutation = useMutation({
    mutationFn: (projectId: string) => adminApi.deleteProject(projectId),
    onSuccess: () => {
      setError(null);
      setDeleting(null);
      invalidate();
    },
    onError: (cause) => {
      setDeleting(null);
      setError(cause instanceof Error ? cause.message : "Unable to delete the project.");
    },
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-muted-foreground">
          Every project on the platform. Archiving pauses new deployments; deleting hides the
          project and stops serving.
        </p>
        <form
          className="flex shrink-0 items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            setQuery(search);
          }}
        >
          <Input
            placeholder="Search name or slug"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            className="h-8 w-52"
          />
          <Button type="submit" size="sm" variant="outline">
            Search
          </Button>
        </form>
      </div>

      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {projectsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {projectsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {projectsQuery.error instanceof Error
            ? projectsQuery.error.message
            : "Unable to load projects."}
        </p>
      )}

      {projectsQuery.data &&
        (projectsQuery.data.projects.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <FolderGit2Icon className="mr-1 inline size-4" />
            No projects found.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Project</TableHead>
                <TableHead>Team</TableHead>
                <TableHead>Latest deployment</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {projectsQuery.data.projects.map((project) => (
                <TableRow key={project.id}>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{project.name}</span>
                      {project.archived_at && <Badge variant="secondary">Archived</Badge>}
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {project.slug} · {project.runtime}
                    </p>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {project.team?.name ?? "—"}
                  </TableCell>
                  <TableCell>
                    {project.latest_deployment ? (
                      <div className="flex flex-wrap items-center gap-1">
                        <span className="text-xs text-muted-foreground">
                          {project.latest_deployment.environment}
                        </span>
                        <BuildStatusBadge status={project.latest_deployment.build_status} />
                        <ReleaseStatusBadge status={project.latest_deployment.release_status} />
                      </div>
                    ) : (
                      <span className="text-sm text-muted-foreground">No deployments</span>
                    )}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(project.created_at).toLocaleDateString()}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex justify-end gap-1">
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => archiveMutation.mutate(project)}
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
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => setDeleting(project)}
                        disabled={deleteMutation.isPending}
                      >
                        <Trash2Icon /> Delete
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}

      <AlertDialog open={deleting !== null} onOpenChange={(open) => !open && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {deleting?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              The project disappears for its team and its sites stop serving. This is a soft delete;
              a database restore stays possible, but treat it as destructive.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => deleting && deleteMutation.mutate(deleting.id)}
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending ? "Deleting…" : "Delete project"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
