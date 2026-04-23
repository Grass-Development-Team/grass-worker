import * as React from "react";
import type { Project } from "@/api/projects";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

type EditProjectFormProps = {
  error: string | null;
  isSaving: boolean;
  onResetError: () => void;
  onSave: (input: { name: string; slug: string }) => void;
  project: Project;
};

export function EditProjectForm({
  error,
  isSaving,
  onResetError,
  onSave,
  project,
}: EditProjectFormProps) {
  const [name, setName] = React.useState(project.name);
  const [slug, setSlug] = React.useState(project.slug);
  const [validationError, setValidationError] = React.useState<string | null>(null);

  React.useEffect(() => {
    setName(project.name);
    setSlug(project.slug);
  }, [project.id, project.name, project.slug]);

  const disabled = project.status === "soft_deleted";
  const formError = validationError ?? error;

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Edit project</h2>
        </CardTitle>
        <CardDescription>
          Update the project display name and slug. Soft-deleted projects must be restored first.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();

            const nextName = name.trim();
            const nextSlug = slug.trim();

            if (!nextName) {
              setValidationError("Project name is required");
              return;
            }

            if (!nextSlug) {
              setValidationError("Project slug is required");
              return;
            }

            setValidationError(null);
            onSave({ name: nextName, slug: nextSlug });
          }}
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="edit-project-name">Project name</Label>
              <Input
                disabled={disabled}
                id="edit-project-name"
                onChange={(event) => {
                  setName(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={name}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-project-slug">Project slug</Label>
              <Input
                disabled={disabled}
                id="edit-project-slug"
                onChange={(event) => {
                  setSlug(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={slug}
              />
            </div>
          </div>
          {formError ? (
            <Alert variant="destructive">
              <AlertTitle>Project update failed</AlertTitle>
              <AlertDescription>{formError}</AlertDescription>
            </Alert>
          ) : null}
          <Button disabled={disabled || isSaving} type="submit">
            {isSaving ? "Saving project..." : "Save project"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
