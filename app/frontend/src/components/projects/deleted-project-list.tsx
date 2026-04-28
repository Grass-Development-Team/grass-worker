import type { Project } from "@/api/projects";
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
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { ProjectStatusBadge } from "./project-status-badge";

type DeletedProjectListProps = {
  actionError: string | null;
  isLoading: boolean;
  onHardDelete: (projectId: string) => void;
  onRestore: (projectId: string, status: "active" | "archived") => void;
  pendingAction: "restore_active" | "restore_archived" | "hard_delete" | null;
  pendingProjectId: string | null;
  projects: Project[];
};

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function sortProjects(projects: Project[]) {
  return [...projects].sort((left, right) => {
    const leftDeletedAt = left.soft_deleted_at ?? left.updated_at;
    const rightDeletedAt = right.soft_deleted_at ?? right.updated_at;

    return (
      new Date(rightDeletedAt).getTime() - new Date(leftDeletedAt).getTime() ||
      left.name.localeCompare(right.name)
    );
  });
}

export function DeletedProjectList({
  actionError,
  isLoading,
  onHardDelete,
  onRestore,
  pendingAction,
  pendingProjectId,
  projects,
}: DeletedProjectListProps) {
  const sortedProjects = sortProjects(projects);

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Recovery queue</h2>
        </CardTitle>
        <CardDescription>Restore soft-deleted projects or remove them permanently.</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {actionError ? (
          <Alert variant="destructive">
            <AlertTitle>Project recovery failed</AlertTitle>
            <AlertDescription>{actionError}</AlertDescription>
          </Alert>
        ) : null}

        {isLoading ? (
          <div className="space-y-3">
            <Skeleton className="h-36" />
            <Skeleton className="h-36" />
          </div>
        ) : sortedProjects.length === 0 ? (
          <Card>
            <CardHeader>
              <CardTitle>No deleted projects</CardTitle>
              <CardDescription>
                Soft-deleted projects will appear here when they need admin recovery.
              </CardDescription>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">
              Restore a project back to active or archived status, or hard delete it to remove
              it permanently.
            </CardContent>
          </Card>
        ) : (
          sortedProjects.map((project) => {
            const isPendingForProject = pendingProjectId === project.id;

            return (
              <Card key={project.id}>
                <CardHeader>
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div className="space-y-1">
                      <CardDescription>{project.slug}</CardDescription>
                      <CardTitle>
                        <h3>{project.name}</h3>
                      </CardTitle>
                      <CardDescription>
                        Deleted on {formatTimestamp(project.soft_deleted_at ?? project.updated_at)}
                      </CardDescription>
                    </div>
                    <ProjectStatusBadge status={project.status} />
                  </div>
                </CardHeader>
                <CardContent className="grid gap-4 text-sm text-muted-foreground lg:grid-cols-[repeat(2,minmax(0,1fr))_auto] lg:items-end">
                  <div className="space-y-1">
                    <p>Created</p>
                    <p className="font-medium text-foreground">
                      {formatTimestamp(project.created_at)}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p>Deleted at</p>
                    <p className="font-medium text-foreground">
                      {formatTimestamp(project.soft_deleted_at ?? project.updated_at)}
                    </p>
                  </div>
                  <div className="flex flex-col gap-3 sm:flex-row lg:justify-end">
                    <Button
                      disabled={Boolean(pendingAction)}
                      onClick={() => onRestore(project.id, "active")}
                      type="button"
                      variant="outline"
                    >
                      {isPendingForProject && pendingAction === "restore_active"
                        ? "Restoring..."
                        : "Restore active"}
                    </Button>
                    <Button
                      disabled={Boolean(pendingAction)}
                      onClick={() => onRestore(project.id, "archived")}
                      type="button"
                      variant="outline"
                    >
                      {isPendingForProject && pendingAction === "restore_archived"
                        ? "Restoring..."
                        : "Restore archived"}
                    </Button>
                    <AlertDialog>
                      <AlertDialogTrigger asChild>
                        <Button
                          disabled={Boolean(pendingAction)}
                          type="button"
                          variant="destructive"
                        >
                          {isPendingForProject && pendingAction === "hard_delete"
                            ? "Deleting..."
                            : "Hard delete project"}
                        </Button>
                      </AlertDialogTrigger>
                      <AlertDialogContent>
                        <AlertDialogHeader>
                          <AlertDialogTitle>Hard delete project?</AlertDialogTitle>
                          <AlertDialogDescription>
                            This permanently removes the project record and cannot be undone.
                          </AlertDialogDescription>
                        </AlertDialogHeader>
                        <AlertDialogFooter>
                          <AlertDialogCancel>Cancel</AlertDialogCancel>
                          <AlertDialogAction
                            className="bg-destructive/10 text-destructive hover:bg-destructive/20"
                            onClick={() => onHardDelete(project.id)}
                          >
                            Hard delete
                          </AlertDialogAction>
                        </AlertDialogFooter>
                      </AlertDialogContent>
                    </AlertDialog>
                  </div>
                </CardContent>
              </Card>
            );
          })
        )}
      </CardContent>
    </Card>
  );
}
