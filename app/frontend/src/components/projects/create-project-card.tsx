import * as React from "react";
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

type CreateProjectCardProps = {
  error: string | null;
  isCreating: boolean;
  onCreateProject: (input: { name: string; slug: string }) => void;
  onResetError: () => void;
  resetToken: number;
};

function deriveSlug(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/-{2,}/g, "-");
}

export function CreateProjectCard({
  error,
  isCreating,
  onCreateProject,
  onResetError,
  resetToken,
}: CreateProjectCardProps) {
  const [projectName, setProjectName] = React.useState("");
  const [projectSlug, setProjectSlug] = React.useState("");
  const [slugTouched, setSlugTouched] = React.useState(false);
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const createError = validationError ?? error;

  React.useEffect(() => {
    setProjectName("");
    setProjectSlug("");
    setSlugTouched(false);
    setValidationError(null);
  }, [resetToken]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Create project</h2>
        </CardTitle>
        <CardDescription>
          Provision a deployment workspace and it will appear in the inventory immediately.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault();

            const name = projectName.trim();
            const slug = projectSlug.trim();

            if (!name) {
              setValidationError("Project name is required");
              return;
            }

            if (!slug) {
              setValidationError("Project slug is required");
              return;
            }

            setValidationError(null);
            onCreateProject({ name, slug });
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="project-name">Project name</Label>
            <Input
              id="project-name"
              onChange={(event) => {
                const value = event.target.value;
                setProjectName(value);
                setValidationError(null);
                onResetError();
                if (!slugTouched) {
                  setProjectSlug(deriveSlug(value));
                }
              }}
              placeholder="Docs Site"
              value={projectName}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="project-slug">Project slug</Label>
            <Input
              id="project-slug"
              onChange={(event) => {
                setSlugTouched(true);
                setProjectSlug(deriveSlug(event.target.value));
                setValidationError(null);
                onResetError();
              }}
              placeholder="docs-site"
              value={projectSlug}
            />
          </div>
          {createError ? (
            <Alert variant="destructive">
              <AlertTitle>Project creation failed</AlertTitle>
              <AlertDescription>{createError}</AlertDescription>
            </Alert>
          ) : null}
          <Button className="w-full" disabled={isCreating} type="submit">
            {isCreating ? "Creating project..." : "Create project"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
