import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useOutletContext } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  hardDeleteProject,
  projectQueryKey,
  projectsListQueryKey,
  projectsQueryKey,
  getProjects,
  restoreProject,
  type Project,
} from "@/api/projects";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { DeletedProjectList } from "@/components/projects/deleted-project-list";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import type { ProtectedOutletContext } from "./protected-route";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function mergeDeletedProject(projects: Project[], project: Project) {
  const otherProjects = projects.filter((item) => item.id !== project.id);
  return [project, ...otherProjects];
}

export function AdminDeletedProjectsPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const queryClient = useQueryClient();
  const deletedProjectsQuery = useQuery({
    queryKey: projectsListQueryKey({ status: "soft_deleted" }),
    queryFn: () => getProjects({ status: "soft_deleted" }),
  });
  const actionMutation = useMutation({
    mutationFn: (
      action:
        | { type: "restore"; projectId: string; status: "active" | "archived" }
        | { type: "hard_delete"; projectId: string },
    ) => {
      if (action.type === "restore") {
        return restoreProject(action.projectId, action.status);
      }

      return hardDeleteProject(action.projectId);
    },
    onSuccess: async (result, action) => {
      const deletedProjectsKey = projectsListQueryKey({ status: "soft_deleted" });

      queryClient.setQueryData<Project[]>(deletedProjectsKey, (current) =>
        (current ?? []).filter((project) => project.id !== action.projectId),
      );

      if (action.type === "restore") {
        const restoredProject = result as Project;
        queryClient.setQueryData(projectQueryKey(action.projectId), restoredProject);
        queryClient.setQueryData<Project[]>(projectsListQueryKey(), (current) =>
          current ? mergeDeletedProject(current, restoredProject) : current,
        );
      } else {
        queryClient.removeQueries({ queryKey: projectQueryKey(action.projectId) });
      }

      await queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <Button
            disabled={deletedProjectsQuery.isPending}
            onClick={() => void deletedProjectsQuery.refetch()}
            type="button"
            variant="outline"
          >
            {deletedProjectsQuery.isPending ? "Refreshing..." : "Refresh deleted projects"}
          </Button>
        }
        description={`Review soft-deleted projects that ${currentUser.email} can recover or remove permanently.`}
        eyebrow="Admin"
        title="Deleted projects"
      />

      {deletedProjectsQuery.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Unable to load deleted projects</AlertTitle>
          <AlertDescription>
            {errorMessage(
              deletedProjectsQuery.error,
              "The deleted project inventory request failed.",
            )}
          </AlertDescription>
        </Alert>
      ) : null}

      {deletedProjectsQuery.isError ? null : (
        <DeletedProjectList
          actionError={
            actionMutation.isError
              ? errorMessage(actionMutation.error, "Unable to update deleted project")
              : null
          }
          isLoading={deletedProjectsQuery.isPending}
          onHardDelete={(projectId) =>
            actionMutation.mutate({ type: "hard_delete", projectId })}
          onRestore={(projectId, status) =>
            actionMutation.mutate({ type: "restore", projectId, status })}
          pendingAction={actionMutation.isPending
            ? actionMutation.variables?.type === "restore"
              ? actionMutation.variables.status === "active"
                ? "restore_active"
                : "restore_archived"
              : "hard_delete"
            : null}
          pendingProjectId={actionMutation.isPending ? actionMutation.variables?.projectId ?? null : null}
          projects={deletedProjectsQuery.data ?? []}
        />
      )}
    </div>
  );
}
