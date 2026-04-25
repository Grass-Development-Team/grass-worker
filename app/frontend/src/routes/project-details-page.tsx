import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useOutletContext, useParams } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  archiveProject,
  getProject,
  projectQueryKey,
  projectsQueryKey,
  restoreProject,
  softDeleteProject,
  transferProjectOwner,
  unarchiveProject,
  updateProject,
  type Project,
} from "@/api/projects";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { DangerZoneCard } from "@/components/projects/danger-zone-card";
import { EditProjectForm } from "@/components/projects/edit-project-form";
import { LifecycleActionsCard } from "@/components/projects/lifecycle-actions-card";
import { ProjectOverviewCard } from "@/components/projects/project-overview-card";
import { projectStatusLabel } from "@/components/projects/project-status-badge";
import { TransferOwnerCard } from "@/components/projects/transfer-owner-card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import type { ProtectedOutletContext } from "./protected-route";

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof ApiError || error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function projectDescription(project: Project) {
  if (project.status === "soft_deleted") {
    return "Deleted project";
  }

  return `${projectStatusLabel(project.status)} project`;
}

export function ProjectDetailsPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const navigate = useNavigate();
  const { projectId } = useParams<{ projectId: string }>();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: projectQueryKey(projectId ?? ""),
    queryFn: () => getProject(projectId ?? ""),
    enabled: Boolean(projectId),
  });

  const setProjectData = (project: Project) => {
    if (!projectId) return;
    queryClient.setQueryData(projectQueryKey(projectId), project);
    queryClient.setQueryData<Project[]>(projectsQueryKey, (current) =>
      current?.map((item) => (item.id === project.id ? project : item)),
    );
  };

  const updateMutation = useMutation({
    mutationFn: (input: { name: string; slug: string }) =>
      updateProject(projectId ?? "", input),
    onSuccess: async (project) => {
      setProjectData(project);
      await queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });
  const lifecycleMutation = useMutation({
    mutationFn: (
      action:
        | { type: "archive" }
        | { type: "unarchive" }
        | { type: "restore"; status: "active" | "archived" },
    ) => {
      if (!projectId) throw new Error("Project id is required");
      if (action.type === "archive") return archiveProject(projectId);
      if (action.type === "unarchive") return unarchiveProject(projectId);
      return restoreProject(projectId, action.status);
    },
    onSuccess: async (project) => {
      setProjectData(project);
      await queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });
  const transferMutation = useMutation({
    mutationFn: (ownerEmail: string) => transferProjectOwner(projectId ?? "", ownerEmail),
    onSuccess: async (project) => {
      setProjectData(project);
      await queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });
  const dangerMutation = useMutation({
    mutationFn: () => {
      if (!projectId) throw new Error("Project id is required");
      return softDeleteProject(projectId);
    },
    onSuccess: async () => {
      queryClient.setQueryData<Project[]>(projectsQueryKey, (current) =>
        current?.filter((item) => item.id !== projectId),
      );
      queryClient.removeQueries({ queryKey: projectQueryKey(projectId ?? "") });
      await navigate("/projects", { replace: true });
    },
  });

  if (!projectId) {
    return (
      <Card className="w-full max-w-lg">
        <CardHeader>
          <CardTitle>
            <h1>Project not found</h1>
          </CardTitle>
          <CardDescription>The requested project identifier is missing.</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  if (query.isPending) {
    return (
      <div className="space-y-6">
        <ConsolePageHeader eyebrow="Project" title="Loading project" />
        <Card>
          <CardHeader>
            <CardTitle>
              <h2>Loading project</h2>
            </CardTitle>
            <CardDescription>Fetching project metadata from the control API.</CardDescription>
          </CardHeader>
        </Card>
      </div>
    );
  }

  if (query.isError) {
    return (
      <div className="space-y-6">
        <ConsolePageHeader
          actions={
            <Button onClick={() => void navigate("/projects")} type="button" variant="outline">
              Back to projects
            </Button>
          }
          eyebrow="Project"
          title="Project unavailable"
        />
        <Alert variant="destructive">
          <AlertTitle>Unable to load project</AlertTitle>
          <AlertDescription>
            {errorMessage(query.error, "Project lookup failed.")}
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  const project = query.data;

  return (
    <div className="space-y-6">
      <ConsolePageHeader
        actions={
          <>
            <Button onClick={() => void navigate("/projects")} type="button" variant="outline">
              Back to projects
            </Button>
            <Button
              disabled={query.isPending}
              onClick={() => void query.refetch()}
              type="button"
              variant="outline"
            >
              {query.isPending ? "Refreshing..." : "Refresh details"}
            </Button>
          </>
        }
        description={projectDescription(project)}
        eyebrow={project.slug}
        title={project.name}
      />

      <ProjectOverviewCard project={project} />

      <EditProjectForm
        error={
          updateMutation.isError
            ? errorMessage(updateMutation.error, "Project update failed")
            : null
        }
        isSaving={updateMutation.isPending}
        onResetError={() => updateMutation.reset()}
        onSave={(input) => updateMutation.mutate(input)}
        project={project}
      />

      <LifecycleActionsCard
        canRestore={currentUser.is_admin}
        error={
          lifecycleMutation.isError
            ? errorMessage(lifecycleMutation.error, "Lifecycle action failed")
            : null
        }
        isPending={lifecycleMutation.isPending}
        onArchive={() => lifecycleMutation.mutate({ type: "archive" })}
        onRestoreToActive={() =>
          lifecycleMutation.mutate({ type: "restore", status: "active" })
        }
        onRestoreToArchived={() =>
          lifecycleMutation.mutate({ type: "restore", status: "archived" })
        }
        onUnarchive={() => lifecycleMutation.mutate({ type: "unarchive" })}
        project={project}
      />

      <TransferOwnerCard
        error={
          transferMutation.isError
            ? errorMessage(transferMutation.error, "Transfer owner failed")
            : null
        }
        isTransferring={transferMutation.isPending}
        onResetError={() => transferMutation.reset()}
        onTransfer={(ownerEmail) => transferMutation.mutate(ownerEmail)}
        project={project}
      />

      <DangerZoneCard
        error={
          dangerMutation.isError
            ? errorMessage(dangerMutation.error, "Danger zone action failed")
            : null
        }
        isPending={dangerMutation.isPending}
        onDelete={() => dangerMutation.mutate()}
        project={project}
      />

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Deployment history</h2>
          </CardTitle>
          <CardDescription>
            This route is ready for deployment records to plug in next.
          </CardDescription>
        </CardHeader>
      </Card>
    </div>
  );
}
