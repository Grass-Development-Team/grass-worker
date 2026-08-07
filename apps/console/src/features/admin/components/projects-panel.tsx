import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArchiveIcon,
  ArchiveRestoreIcon,
  ChevronDownIcon,
  FolderGit2Icon,
  Settings2Icon,
  Trash2Icon,
} from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";

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
import { Checkbox } from "@/components/ui/checkbox";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
import { showBatchResultToast } from "../batch-result-toast";

export function ProjectsPanel() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<AdminProject["status"] | undefined>();
  const [deleting, setDeleting] = useState<AdminProject | null>(null);
  const [confirmingBatchDelete, setConfirmingBatchDelete] = useState(false);
  const [selectedProjectIds, setSelectedProjectIds] = useState<Set<string>>(new Set());

  const filters = {
    ...(query ? { q: query } : {}),
    ...(status ? { status } : {}),
  };

  const projectsQuery = useQuery({
    queryKey: ["admin", "projects", filters],
    queryFn: () => adminApi.listProjects(filters),
  });
  const visibleProjectIds = projectsQuery.data?.projects.map((project) => project.id) ?? [];
  const selectedVisibleCount = visibleProjectIds.filter((id) => selectedProjectIds.has(id)).length;
  const allVisibleSelected =
    visibleProjectIds.length > 0 && selectedVisibleCount === visibleProjectIds.length;

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["admin", "projects"] });

  const archiveMutation = useMutation({
    mutationFn: (project: AdminProject) =>
      project.archived_at
        ? adminApi.unarchiveProject(project.id)
        : adminApi.archiveProject(project.id),
    onSuccess: invalidate,
  });

  const deleteMutation = useMutation({
    mutationFn: (projectId: string) => adminApi.deleteProject(projectId),
    onSuccess: () => {
      setDeleting(null);
      invalidate();
    },
    onError: () => setDeleting(null),
  });

  const restoreMutation = useMutation({
    mutationFn: (projectId: string) => adminApi.restoreProject(projectId),
    onSuccess: invalidate,
  });

  const batchMutation = useMutation({
    mutationFn: (action: "archive" | "unarchive" | "delete") =>
      adminApi.batchProjects({ action, ids: [...selectedProjectIds] }),
    onSuccess: ({ results }, action) => {
      showBatchResultToast(
        results,
        results.length === 1 ? "project" : "projects",
        action === "archive" ? "archived" : action === "unarchive" ? "restored" : "deleted",
      );
      setConfirmingBatchDelete(false);
      setSelectedProjectIds(new Set());
      invalidate();
    },
    onError: () => setConfirmingBatchDelete(false),
  });

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <p className="text-sm text-muted-foreground">
          Every project on the platform. Archiving pauses new deployments; deleting hides the
          project and stops serving.
        </p>
        <div className="flex flex-wrap items-center justify-end gap-2">
          <form
            className="flex shrink-0 items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              setQuery(search);
              setSelectedProjectIds(new Set());
            }}
          >
            <Input
              aria-label="Search projects"
              placeholder="Search name or slug"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              className="h-8 w-52"
            />
            <Button type="submit" size="sm" variant="outline">
              Search
            </Button>
          </form>
          <Select
            value={status ?? "all"}
            onValueChange={(value) => {
              setStatus(value === "all" ? undefined : (value as AdminProject["status"]));
              setSelectedProjectIds(new Set());
            }}
          >
            <SelectTrigger aria-label="Project status" size="sm">
              <SelectValue placeholder="All statuses" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All statuses</SelectItem>
                <SelectItem value="active">Active</SelectItem>
                <SelectItem value="archived">Archived</SelectItem>
                <SelectItem value="deleted">Deleted</SelectItem>
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
      </div>

      {selectedProjectIds.size > 0 && (
        <div className="flex min-h-10 items-center justify-between gap-4 border-y px-1 py-2">
          <p className="text-sm font-medium">{selectedProjectIds.size} projects selected</p>
          <div className="flex items-center gap-2">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  aria-label="Bulk actions"
                  disabled={batchMutation.isPending}
                >
                  Bulk actions <ChevronDownIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onClick={() => batchMutation.mutate("archive")}>
                  <ArchiveIcon data-icon="inline-start" /> Archive selected
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => batchMutation.mutate("unarchive")}>
                  <ArchiveRestoreIcon data-icon="inline-start" /> Unarchive selected
                </DropdownMenuItem>
                <DropdownMenuItem
                  variant="destructive"
                  onClick={() => setConfirmingBatchDelete(true)}
                >
                  <Trash2Icon data-icon="inline-start" /> Delete selected
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => setSelectedProjectIds(new Set())}
            >
              Clear selection
            </Button>
          </div>
        </div>
      )}

      {projectsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}

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
                <TableHead className="w-10">
                  <Checkbox
                    aria-label="Select all visible projects"
                    checked={
                      allVisibleSelected ? true : selectedVisibleCount > 0 ? "indeterminate" : false
                    }
                    onCheckedChange={(checked) =>
                      setSelectedProjectIds((current) => {
                        const next = new Set(current);
                        for (const id of visibleProjectIds) {
                          if (checked === true) next.add(id);
                          else next.delete(id);
                        }
                        return next;
                      })
                    }
                  />
                </TableHead>
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
                    <Checkbox
                      aria-label={`Select ${project.name}`}
                      checked={selectedProjectIds.has(project.id)}
                      onCheckedChange={(checked) =>
                        setSelectedProjectIds((current) => {
                          const next = new Set(current);
                          if (checked === true) next.add(project.id);
                          else next.delete(project.id);
                          return next;
                        })
                      }
                    />
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{project.name}</span>
                      {project.status === "archived" && <Badge variant="secondary">Archived</Badge>}
                      {project.status === "deleted" && <Badge variant="destructive">Deleted</Badge>}
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
                      <Button size="sm" variant="outline" asChild>
                        <Link to={`/admin/projects/${project.id}`}>
                          <Settings2Icon data-icon="inline-start" />
                          Manage
                        </Link>
                      </Button>
                      {project.status === "deleted" ? (
                        <Button
                          size="sm"
                          variant="outline"
                          aria-label={`Restore ${project.name}`}
                          onClick={() => restoreMutation.mutate(project.id)}
                          disabled={restoreMutation.isPending}
                        >
                          <ArchiveRestoreIcon data-icon="inline-start" /> Restore
                        </Button>
                      ) : (
                        <>
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => archiveMutation.mutate(project)}
                            disabled={archiveMutation.isPending}
                          >
                            {project.status === "archived" ? (
                              <>
                                <ArchiveRestoreIcon data-icon="inline-start" /> Unarchive
                              </>
                            ) : (
                              <>
                                <ArchiveIcon data-icon="inline-start" /> Archive
                              </>
                            )}
                          </Button>
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => setDeleting(project)}
                            disabled={deleteMutation.isPending}
                          >
                            <Trash2Icon data-icon="inline-start" /> Delete
                          </Button>
                        </>
                      )}
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
              {deleteMutation.isPending ? "Deleting…" : "Delete Project"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={confirmingBatchDelete}
        onOpenChange={(open) => !open && setConfirmingBatchDelete(false)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {selectedProjectIds.size} projects?</AlertDialogTitle>
            <AlertDialogDescription>
              The selected projects disappear for their teams and stop serving. Administrators can
              restore these soft-deleted projects later.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => batchMutation.mutate("delete")}
              disabled={batchMutation.isPending}
            >
              {batchMutation.isPending ? "Deleting…" : "Delete Projects"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
