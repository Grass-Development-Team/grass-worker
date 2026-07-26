import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { SettingsCard } from "@/components/settings-card";

import { projectsApi, type UpdateProjectInput } from "./projects.api";
import { useProject } from "./project-layout";

export function ProjectSettingsBuildRoute() {
  const { project } = useProject();
  const queryClient = useQueryClient();

  const [repositoryUrl, setRepositoryUrl] = useState(project.repository_url ?? "");
  const [defaultBranch, setDefaultBranch] = useState(project.default_branch ?? "");
  const [rootDirectory, setRootDirectory] = useState(project.source_config.root_directory ?? "");
  const [installCommand, setInstallCommand] = useState(project.install_command ?? "");
  const [buildCommand, setBuildCommand] = useState(project.build_command ?? "");
  const [outputDirectory, setOutputDirectory] = useState(project.output_directory ?? "");
  const [gitError, setGitError] = useState<string | null>(null);
  const [buildError, setBuildError] = useState<string | null>(null);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["project", project.id] });

  const gitMutation = useMutation({
    mutationFn: () =>
      projectsApi.update(project.id, {
        repository_url: repositoryUrl,
        default_branch: defaultBranch,
      } satisfies UpdateProjectInput),
    onSuccess: () => {
      setGitError(null);
      invalidate();
    },
    onError: (cause) =>
      setGitError(cause instanceof Error ? cause.message : "Unable to save Git settings."),
  });

  const buildMutation = useMutation({
    mutationFn: () =>
      projectsApi.update(project.id, {
        root_directory: rootDirectory,
        install_command: installCommand,
        build_command: buildCommand,
        output_directory: outputDirectory,
      } satisfies UpdateProjectInput),
    onSuccess: () => {
      setBuildError(null);
      invalidate();
    },
    onError: (cause) =>
      setBuildError(cause instanceof Error ? cause.message : "Unable to save build settings."),
  });

  return (
    <div className="space-y-6">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          gitMutation.mutate();
        }}
      >
        <SettingsCard
          title="Git Repository"
          description="Deployments clone this repository at build time."
          hint="Public repositories only in the first stage."
          action={
            <Button type="submit" size="sm" disabled={gitMutation.isPending}>
              {gitMutation.isPending ? "Saving…" : "Save"}
            </Button>
          }
        >
          <div className="grid gap-4 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
            <Field>
              <FieldLabel htmlFor="settings-repo">Repository URL</FieldLabel>
              <Input
                id="settings-repo"
                placeholder="https://github.com/acme/site.git"
                value={repositoryUrl}
                onChange={(event) => setRepositoryUrl(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-branch">Production branch</FieldLabel>
              <Input
                id="settings-branch"
                placeholder="main"
                value={defaultBranch}
                onChange={(event) => setDefaultBranch(event.target.value)}
              />
            </Field>
          </div>
          {gitError && (
            <p role="alert" className="mt-2 text-sm text-destructive">
              {gitError}
            </p>
          )}
        </SettingsCard>
      </form>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          buildMutation.mutate();
        }}
      >
        <SettingsCard
          title="Build &amp; Output Settings"
          description="How the deployable output is produced inside the build container."
          hint="Leave fields empty to auto-detect from the framework."
          action={
            <Button type="submit" size="sm" disabled={buildMutation.isPending}>
              {buildMutation.isPending ? "Saving…" : "Save"}
            </Button>
          }
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="settings-root">Root directory</FieldLabel>
              <Input
                id="settings-root"
                placeholder="."
                value={rootDirectory}
                onChange={(event) => setRootDirectory(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-install">Install command</FieldLabel>
              <Input
                id="settings-install"
                placeholder="npm install"
                value={installCommand}
                onChange={(event) => setInstallCommand(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-build">Build command</FieldLabel>
              <Input
                id="settings-build"
                placeholder="npm run build"
                value={buildCommand}
                onChange={(event) => setBuildCommand(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-output">Output directory</FieldLabel>
              <Input
                id="settings-output"
                placeholder="dist"
                value={outputDirectory}
                onChange={(event) => setOutputDirectory(event.target.value)}
              />
            </Field>
          </div>
          {buildError && (
            <p role="alert" className="mt-2 text-sm text-destructive">
              {buildError}
            </p>
          )}
        </SettingsCard>
      </form>
    </div>
  );
}
