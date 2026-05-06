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
  onSave: (input: {
    name: string;
    slug: string;
    repository_url: string;
    production_branch: string;
    root_directory?: string | null;
    install_command?: string | null;
    build_command?: string | null;
    output_directory?: string | null;
  }) => void;
  project: Project;
};

function normalizeOptionalInput(value: string) {
  const normalized = value.trim();
  return normalized.length > 0 ? normalized : null;
}

export function EditProjectForm({
  error,
  isSaving,
  onResetError,
  onSave,
  project,
}: EditProjectFormProps) {
  const [name, setName] = React.useState(project.name);
  const [slug, setSlug] = React.useState(project.slug);
  const [repositoryUrl, setRepositoryUrl] = React.useState(project.repository_url ?? "");
  const [productionBranch, setProductionBranch] = React.useState(
    project.production_branch ?? "main",
  );
  const [rootDirectory, setRootDirectory] = React.useState(project.root_directory ?? "");
  const [installCommand, setInstallCommand] = React.useState(
    project.install_command ?? "bun install",
  );
  const [buildCommand, setBuildCommand] = React.useState(
    project.build_command ?? "bun run build",
  );
  const [outputDirectory, setOutputDirectory] = React.useState(
    project.output_directory ?? "dist",
  );
  const [validationError, setValidationError] = React.useState<string | null>(null);

  React.useEffect(() => {
    setName(project.name);
    setSlug(project.slug);
    setRepositoryUrl(project.repository_url ?? "");
    setProductionBranch(project.production_branch ?? "main");
    setRootDirectory(project.root_directory ?? "");
    setInstallCommand(project.install_command ?? "bun install");
    setBuildCommand(project.build_command ?? "bun run build");
    setOutputDirectory(project.output_directory ?? "dist");
  }, [
    project.id,
    project.name,
    project.slug,
    project.repository_url,
    project.production_branch,
    project.root_directory,
    project.install_command,
    project.build_command,
    project.output_directory,
  ]);

  const disabled = project.status === "soft_deleted";
  const formError = validationError ?? error;

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Edit project</h2>
        </CardTitle>
        <CardDescription>
          Update the repository import and build settings. Soft-deleted projects must be restored first.
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

            const nextRepositoryUrl = repositoryUrl.trim();
            if (!nextRepositoryUrl) {
              setValidationError("Git repository URL is required");
              return;
            }

            const nextProductionBranch = productionBranch.trim();
            if (!nextProductionBranch) {
              setValidationError("Production branch is required");
              return;
            }

            setValidationError(null);
            onSave({
              name: nextName,
              slug: nextSlug,
              repository_url: nextRepositoryUrl,
              production_branch: nextProductionBranch,
              root_directory: normalizeOptionalInput(rootDirectory),
              install_command: normalizeOptionalInput(installCommand) ?? "bun install",
              build_command: normalizeOptionalInput(buildCommand) ?? "bun run build",
              output_directory: normalizeOptionalInput(outputDirectory) ?? "dist",
            });
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
            <div className="space-y-2 sm:col-span-2">
              <Label htmlFor="edit-project-repository-url">Git repository URL</Label>
              <Input
                disabled={disabled}
                id="edit-project-repository-url"
                onChange={(event) => {
                  setRepositoryUrl(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={repositoryUrl}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-project-production-branch">Production branch</Label>
              <Input
                disabled={disabled}
                id="edit-project-production-branch"
                onChange={(event) => {
                  setProductionBranch(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={productionBranch}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-project-root-directory">Root directory</Label>
              <Input
                disabled={disabled}
                id="edit-project-root-directory"
                onChange={(event) => {
                  setRootDirectory(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={rootDirectory}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-project-install-command">Install command</Label>
              <Input
                disabled={disabled}
                id="edit-project-install-command"
                onChange={(event) => {
                  setInstallCommand(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={installCommand}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-project-build-command">Build command</Label>
              <Input
                disabled={disabled}
                id="edit-project-build-command"
                onChange={(event) => {
                  setBuildCommand(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={buildCommand}
              />
            </div>
            <div className="space-y-2 sm:col-span-2">
              <Label htmlFor="edit-project-output-directory">Output directory</Label>
              <Input
                disabled={disabled}
                id="edit-project-output-directory"
                onChange={(event) => {
                  setOutputDirectory(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                value={outputDirectory}
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
