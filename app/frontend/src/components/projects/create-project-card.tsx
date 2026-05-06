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
  onCreateProject: (input: {
    name: string;
    slug: string;
    repository_url: string;
    production_branch: string;
    root_directory?: string | null;
    install_command?: string | null;
    build_command?: string | null;
    output_directory?: string | null;
  }) => void;
  onResetError: () => void;
  resetToken: number;
};

function normalizeOptionalInput(value: string) {
  const normalized = value.trim();
  return normalized.length > 0 ? normalized : null;
}

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
  const [repositoryUrl, setRepositoryUrl] = React.useState("");
  const [productionBranch, setProductionBranch] = React.useState("main");
  const [rootDirectory, setRootDirectory] = React.useState("");
  const [installCommand, setInstallCommand] = React.useState("bun install");
  const [buildCommand, setBuildCommand] = React.useState("bun run build");
  const [outputDirectory, setOutputDirectory] = React.useState("dist");
  const [slugTouched, setSlugTouched] = React.useState(false);
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const createError = validationError ?? error;

  React.useEffect(() => {
    setProjectName("");
    setProjectSlug("");
    setRepositoryUrl("");
    setProductionBranch("main");
    setRootDirectory("");
    setInstallCommand("bun install");
    setBuildCommand("bun run build");
    setOutputDirectory("dist");
    setSlugTouched(false);
    setValidationError(null);
  }, [resetToken]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>
          <h2>Import project</h2>
        </CardTitle>
        <CardDescription>
          Import a public GitHub repository and save the production build settings used for deployments.
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

            const normalizedRepositoryUrl = repositoryUrl.trim();
            if (!normalizedRepositoryUrl) {
              setValidationError("Git repository URL is required");
              return;
            }

            const normalizedProductionBranch = productionBranch.trim();
            if (!normalizedProductionBranch) {
              setValidationError("Production branch is required");
              return;
            }

            setValidationError(null);
            onCreateProject({
              name,
              slug,
              repository_url: normalizedRepositoryUrl,
              production_branch: normalizedProductionBranch,
              root_directory: normalizeOptionalInput(rootDirectory),
              install_command: normalizeOptionalInput(installCommand) ?? "bun install",
              build_command: normalizeOptionalInput(buildCommand) ?? "bun run build",
              output_directory: normalizeOptionalInput(outputDirectory) ?? "dist",
            });
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
          <div className="space-y-2">
            <Label htmlFor="project-repository-url">Git repository URL</Label>
            <Input
              id="project-repository-url"
              onChange={(event) => {
                setRepositoryUrl(event.target.value);
                setValidationError(null);
                onResetError();
              }}
              placeholder="https://github.com/acme/docs-site"
              value={repositoryUrl}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="project-production-branch">Production branch</Label>
            <Input
              id="project-production-branch"
              onChange={(event) => {
                setProductionBranch(event.target.value);
                setValidationError(null);
                onResetError();
              }}
              placeholder="main"
              value={productionBranch}
            />
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="project-root-directory">Root directory</Label>
              <Input
                id="project-root-directory"
                onChange={(event) => {
                  setRootDirectory(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                placeholder="apps/docs"
                value={rootDirectory}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="project-output-directory">Output directory</Label>
              <Input
                id="project-output-directory"
                onChange={(event) => {
                  setOutputDirectory(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                placeholder="dist"
                value={outputDirectory}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="project-install-command">Install command</Label>
              <Input
                id="project-install-command"
                onChange={(event) => {
                  setInstallCommand(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                placeholder="bun install"
                value={installCommand}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="project-build-command">Build command</Label>
              <Input
                id="project-build-command"
                onChange={(event) => {
                  setBuildCommand(event.target.value);
                  setValidationError(null);
                  onResetError();
                }}
                placeholder="bun run build"
                value={buildCommand}
              />
            </div>
          </div>
          {createError ? (
            <Alert variant="destructive">
              <AlertTitle>Project import failed</AlertTitle>
              <AlertDescription>{createError}</AlertDescription>
            </Alert>
          ) : null}
          <Button className="w-full" disabled={isCreating} type="submit">
            {isCreating ? "Importing project..." : "Import project"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
