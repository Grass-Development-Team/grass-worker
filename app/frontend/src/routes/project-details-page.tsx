import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { useNavigate, useOutletContext, useParams } from "react-router-dom";
import { ApiError } from "@/api/client";
import {
  createProjectDeployment,
  deploymentQueryKey,
  deploymentsQueryKey,
  getProjectDeployments,
  type Deployment,
} from "@/api/deployments";
import {
  archiveProject,
  getProject,
  projectQueryKey,
  projectsListQueryKey,
  projectsQueryKey,
  restoreProject,
  softDeleteProject,
  transferProjectOwner,
  unarchiveProject,
  updateProject,
  type Project,
} from "@/api/projects";
import {
  getProjectRelease,
  projectReleaseQueryKey,
  releasePublicUrl,
  rollbackProjectRelease,
} from "@/api/releases";
import { ConsolePageHeader } from "@/components/console/console-page-header";
import { CreateDeploymentCard } from "@/components/deployments/create-deployment-card";
import { DeploymentList } from "@/components/deployments/deployment-list";
import { DangerZoneCard } from "@/components/projects/danger-zone-card";
import { EditProjectForm } from "@/components/projects/edit-project-form";
import { LifecycleActionsCard } from "@/components/projects/lifecycle-actions-card";
import { ProjectHostBindingsCard } from "@/components/projects/project-host-bindings-card";
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

function releaseDeploymentLabel(sourceRevision: string | null, sourceBranch: string | null) {
  return sourceRevision ?? sourceBranch ?? "Manual deployment";
}

export function ProjectDetailsPage() {
  const { currentUser } = useOutletContext<ProtectedOutletContext>();
  const navigate = useNavigate();
  const { projectId } = useParams<{ projectId: string }>();
  const queryClient = useQueryClient();
  const [deploymentResetToken, setDeploymentResetToken] = React.useState(0);
  const activeProjectsListQueryKey = projectsListQueryKey();
  const deletedProjectsListQueryKey = projectsListQueryKey({ status: "soft_deleted" });
  const query = useQuery({
    queryKey: projectQueryKey(projectId ?? ""),
    queryFn: () => getProject(projectId ?? ""),
    enabled: Boolean(projectId),
  });
  const deploymentsQuery = useQuery({
    queryKey: deploymentsQueryKey(projectId ?? ""),
    queryFn: () => getProjectDeployments(projectId ?? ""),
    enabled: Boolean(projectId),
  });
  const releaseQuery = useQuery({
    queryKey: projectReleaseQueryKey(projectId ?? ""),
    queryFn: () => getProjectRelease(projectId ?? ""),
    enabled: Boolean(projectId),
    retry: false,
  });

  const setProjectData = (project: Project) => {
    if (!projectId) return;
    queryClient.setQueryData(projectQueryKey(projectId), project);
    queryClient.setQueryData<Project[]>(activeProjectsListQueryKey, (current) =>
      current?.map((item) => (item.id === project.id ? project : item)),
    );
  };

  const updateMutation = useMutation({
    mutationFn: (input: {
      name: string;
      slug: string;
      repository_url: string;
      production_branch: string;
      root_directory?: string | null;
      install_command?: string | null;
      build_command?: string | null;
      output_directory?: string | null;
    }) =>
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
    onSuccess: async (project, action) => {
      setProjectData(project);
      if (action.type === "restore") {
        queryClient.setQueryData<Project[]>(deletedProjectsListQueryKey, (current) =>
          (current ?? []).filter((item) => item.id !== project.id),
        );
      }
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
    onSuccess: async (project) => {
      queryClient.setQueryData<Project[]>(activeProjectsListQueryKey, (current) =>
        current?.filter((item) => item.id !== projectId),
      );
      queryClient.setQueryData<Project[]>(deletedProjectsListQueryKey, (current) => {
        const remainingProjects = (current ?? []).filter((item) => item.id !== project.id);
        return [project, ...remainingProjects];
      });
      queryClient.removeQueries({ queryKey: projectQueryKey(projectId ?? "") });
      await queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      await navigate("/projects", { replace: true });
    },
  });
  const createDeploymentMutation = useMutation({
    mutationFn: (input: { source_branch?: string; source_revision?: string }) =>
      createProjectDeployment(projectId ?? "", input),
    onSuccess: async (deployment) => {
      queryClient.setQueryData<Deployment[]>(
        deploymentsQueryKey(projectId ?? ""),
        (current) => [deployment, ...(current ?? []).filter((item) => item.id !== deployment.id)],
      );
      queryClient.setQueryData(deploymentQueryKey(projectId ?? "", deployment.id), deployment);
      setDeploymentResetToken((current) => current + 1);
      await deploymentsQuery.refetch();
    },
  });
  const rollbackReleaseMutation = useMutation({
    mutationFn: () => rollbackProjectRelease(projectId ?? ""),
    onSuccess: async (release) => {
      if (!projectId) return;

      queryClient.setQueryData(projectReleaseQueryKey(projectId), release);
      await queryClient.invalidateQueries({ queryKey: projectReleaseQueryKey(projectId) });
    },
  });

  const refreshProjectRelease = async () => {
    await releaseQuery.refetch();
  };

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
  const deploymentCreateError = createDeploymentMutation.isError
    ? errorMessage(createDeploymentMutation.error, "Unable to create deployment")
    : null;
  const deploymentCreateDisabled = project.status !== "active" || dangerMutation.isPending;
  const deploymentCreateDisabledReason =
    project.status === "archived"
      ? "Archived projects cannot create new deployments."
      : project.status === "soft_deleted"
        ? "Restore the project before creating deployments."
        : null;
  const liveRelease = releaseQuery.data?.active_deployment;
  const liveSiteUrl = releasePublicUrl(releaseQuery.data?.primary_host ?? null);
  const canRollbackRelease = Boolean(releaseQuery.data?.rollback_deployment_id);

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

      <ProjectHostBindingsCard
        currentUserIsAdmin={currentUser.is_admin}
        onHostsChanged={refreshProjectRelease}
        projectId={projectId}
      />

      {releaseQuery.data ? (
        <Card>
          <CardHeader>
            <CardTitle>
              <h2>Live release</h2>
            </CardTitle>
            <CardDescription>
              The deployment currently mapped to the primary public host for this project.
            </CardDescription>
          </CardHeader>
          <div className="px-6 pb-6">
            {liveRelease ? (
              <div className="space-y-4">
                <div className="space-y-1">
                  <p className="text-sm text-muted-foreground">Current deployment</p>
                  <p className="font-medium text-foreground">
                    {releaseDeploymentLabel(
                      liveRelease.source_revision,
                      liveRelease.source_branch,
                    )}
                  </p>
                </div>
                <div className="flex flex-wrap gap-3">
                  {liveSiteUrl ? (
                    <Button asChild type="button" variant="outline">
                      <a href={liveSiteUrl} rel="noreferrer" target="_blank">
                        Open live site
                      </a>
                    </Button>
                  ) : (
                    <Button disabled type="button" variant="outline">
                      Open live site
                    </Button>
                  )}
                  <Button
                    disabled={!canRollbackRelease || rollbackReleaseMutation.isPending}
                    onClick={() => rollbackReleaseMutation.mutate()}
                    type="button"
                    variant="outline"
                  >
                    {rollbackReleaseMutation.isPending
                      ? "Rolling back..."
                      : "Roll back release"}
                  </Button>
                </div>
                {rollbackReleaseMutation.isError ? (
                  <Alert variant="destructive">
                    <AlertTitle>Release rollback failed</AlertTitle>
                    <AlertDescription>
                      {errorMessage(
                        rollbackReleaseMutation.error,
                        "Unable to roll back the live release.",
                      )}
                    </AlertDescription>
                  </Alert>
                ) : null}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                No deployment is live yet for this project.
              </p>
            )}
          </div>
        </Card>
      ) : null}

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

      {deploymentsQuery.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Unable to load deployments</AlertTitle>
          <AlertDescription>
            {errorMessage(deploymentsQuery.error, "The deployment history request failed.")}
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(320px,1fr)]">
        <DeploymentList
          deployments={deploymentsQuery.data ?? []}
          isLoading={deploymentsQuery.isPending}
          onViewDeployment={(deploymentId) =>
            void navigate(`/projects/${projectId}/deployments/${deploymentId}`)
          }
        />
        <CreateDeploymentCard
          disabled={deploymentCreateDisabled}
          disabledReason={deploymentCreateDisabledReason}
          error={deploymentCreateError}
          isCreating={createDeploymentMutation.isPending}
          onCreateDeployment={(input) => createDeploymentMutation.mutate(input)}
          onResetError={() => createDeploymentMutation.reset()}
          resetToken={deploymentResetToken}
        />
      </div>

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
    </div>
  );
}
