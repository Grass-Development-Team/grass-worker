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

type LifecycleActionsCardProps = {
  canRestore: boolean;
  error: string | null;
  isPending: boolean;
  onArchive: () => void;
  onRestoreToActive: () => void;
  onRestoreToArchived: () => void;
  onUnarchive: () => void;
  project: Project;
};

export function LifecycleActionsCard({
  canRestore,
  error,
  isPending,
  onArchive,
  onRestoreToActive,
  onRestoreToArchived,
  onUnarchive,
  project,
}: LifecycleActionsCardProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Lifecycle</h2>
        </CardTitle>
        <CardDescription>
          Move projects between deployable, archived, and administrator-restored states.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {error ? (
          <Alert variant="destructive">
            <AlertTitle>Lifecycle action failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        <div className="flex flex-col gap-3 sm:flex-row">
          {project.status === "active" ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button disabled={isPending} type="button" variant="outline">
                  Archive project
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Archive project?</AlertDialogTitle>
                  <AlertDialogDescription>
                    Archived projects are kept for history but stop accepting normal deployment
                    work.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={onArchive}>Archive</AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          ) : null}

          {project.status === "archived" ? (
            <Button disabled={isPending} onClick={onUnarchive} type="button" variant="outline">
              Unarchive project
            </Button>
          ) : null}

          {project.status === "soft_deleted" && canRestore ? (
            <>
              <Button
                disabled={isPending}
                onClick={onRestoreToActive}
                type="button"
                variant="outline"
              >
                Restore active
              </Button>
              <Button
                disabled={isPending}
                onClick={onRestoreToArchived}
                type="button"
                variant="outline"
              >
                Restore archived
              </Button>
            </>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}
