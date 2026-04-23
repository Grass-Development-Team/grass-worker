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

type DangerZoneCardProps = {
  canHardDelete: boolean;
  error: string | null;
  isPending: boolean;
  onHardDelete: () => void;
  onSoftDelete: () => void;
  project: Project;
};

export function DangerZoneCard({
  canHardDelete,
  error,
  isPending,
  onHardDelete,
  onSoftDelete,
  project,
}: DangerZoneCardProps) {
  return (
    <Card className="border-destructive/30">
      <CardHeader>
        <CardTitle>
          <h2>Danger Zone</h2>
        </CardTitle>
        <CardDescription>
          Soft delete hides the project from normal users. Administrator hard delete permanently
          removes a soft-deleted project.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {error ? (
          <Alert variant="destructive">
            <AlertTitle>Danger zone action failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        <div className="flex flex-col gap-3 sm:flex-row">
          {project.status !== "soft_deleted" ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button disabled={isPending} type="button" variant="destructive">
                  Soft delete project
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Soft delete project?</AlertDialogTitle>
                  <AlertDialogDescription>
                    Normal users will no longer see this project. Administrators can still restore
                    or permanently delete it.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    className="bg-destructive/10 text-destructive hover:bg-destructive/20"
                    onClick={onSoftDelete}
                  >
                    Soft delete
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          ) : null}

          {canHardDelete ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button disabled={isPending} type="button" variant="destructive">
                  Hard delete
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Hard delete project?</AlertDialogTitle>
                  <AlertDialogDescription>
                    This permanently removes the soft-deleted project and cannot be undone.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    className="bg-destructive/10 text-destructive hover:bg-destructive/20"
                    onClick={onHardDelete}
                  >
                    Hard delete
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}
