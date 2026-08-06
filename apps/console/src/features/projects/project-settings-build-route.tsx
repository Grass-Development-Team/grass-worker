import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SettingsCard } from "@/components/settings-card";
import { canContributeToProjects, canManageMembers } from "../teams/team-permissions";
import { teamsApi, type SourceCredential } from "../teams/teams.api";

import { projectsApi, type UpdateProjectInput } from "./projects.api";
import { useProject } from "./project-layout";

export function ProjectSettingsBuildRoute() {
  const { project, role } = useProject();
  const queryClient = useQueryClient();

  const [repositoryUrl, setRepositoryUrl] = useState(project.repository_url ?? "");
  const [defaultBranch, setDefaultBranch] = useState(project.default_branch ?? "");
  const [rootDirectory, setRootDirectory] = useState(project.source_config.root_directory ?? "");
  const [installCommand, setInstallCommand] = useState(project.install_command ?? "");
  const [buildCommand, setBuildCommand] = useState(project.build_command ?? "");
  const [outputDirectory, setOutputDirectory] = useState(project.output_directory ?? "");
  const [selectedCredentialId, setSelectedCredentialId] = useState("none");
  const canEdit = canContributeToProjects(role);
  const canManageCredentials = canManageMembers(role);

  const boundCredential = useQuery({
    queryKey: ["project-source-credential", project.id],
    queryFn: () => projectsApi.getSourceCredential(project.id),
  });
  const credentials = useQuery({
    queryKey: ["source-credentials", project.team_id],
    queryFn: () => teamsApi.listSourceCredentials(project.team_id),
    enabled: canManageCredentials,
  });
  useEffect(() => {
    setSelectedCredentialId(boundCredential.data?.credential?.id ?? "none");
  }, [boundCredential.data]);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["project", project.id] });

  const gitMutation = useMutation({
    mutationFn: () =>
      projectsApi.update(project.id, {
        repository_url: repositoryUrl,
        default_branch: defaultBranch,
      } satisfies UpdateProjectInput),
    onSuccess: invalidate,
  });

  const buildMutation = useMutation({
    mutationFn: () =>
      projectsApi.update(project.id, {
        root_directory: rootDirectory,
        install_command: installCommand,
        build_command: buildCommand,
        output_directory: outputDirectory,
      } satisfies UpdateProjectInput),
    onSuccess: invalidate,
  });

  const credentialMutation = useMutation({
    mutationFn: () =>
      selectedCredentialId === "none"
        ? projectsApi.unbindSourceCredential(project.id)
        : projectsApi.bindSourceCredential(project.id, selectedCredentialId),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["project-source-credential", project.id] }),
        invalidate(),
      ]);
    },
  });

  const compatibleCredentials = (credentials.data?.credentials ?? []).filter(
    (credential) =>
      !credential.revoked_at && credentialMatchesRepository(credential, project.repository_url),
  );
  const repositoryChanged = repositoryUrl.trim() !== (project.repository_url ?? "");

  return (
    <div className="space-y-6">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (canEdit) gitMutation.mutate();
        }}
      >
        <SettingsCard
          title="Git Repository"
          description="Deployments clone this repository at build time."
          hint="HTTP, HTTPS, SSH, scp-like SSH, and git:// URLs are supported; bind a credential for private HTTPS or SSH access."
          action={
            canEdit ? (
              <Button type="submit" size="sm" disabled={gitMutation.isPending}>
                {gitMutation.isPending ? "Saving…" : "Save"}
              </Button>
            ) : undefined
          }
        >
          <div className="grid gap-4 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
            <Field>
              <FieldLabel htmlFor="settings-repo">Repository URL</FieldLabel>
              <Input
                id="settings-repo"
                placeholder="https://github.com/acme/site.git"
                value={repositoryUrl}
                readOnly={!canEdit}
                onChange={(event) => setRepositoryUrl(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-branch">Production branch</FieldLabel>
              <Input
                id="settings-branch"
                placeholder="main"
                value={defaultBranch}
                readOnly={!canEdit}
                onChange={(event) => setDefaultBranch(event.target.value)}
              />
            </Field>
          </div>
          <div className="mt-6 border-t pt-4">
            <Field>
              <FieldLabel>Private repository credential</FieldLabel>
              {canManageCredentials ? (
                <div className="flex flex-col gap-2 sm:flex-row">
                  <Select value={selectedCredentialId} onValueChange={setSelectedCredentialId}>
                    <SelectTrigger className="w-full sm:max-w-md">
                      <SelectValue placeholder="Anonymous access" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">Anonymous access</SelectItem>
                      {compatibleCredentials.map((credential) => (
                        <SelectItem key={credential.id} value={credential.id}>
                          {credential.name} · {credential.username}@{credential.host}:
                          {credential.port}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Button
                    type="button"
                    variant="outline"
                    disabled={credentialMutation.isPending || repositoryChanged}
                    onClick={() => credentialMutation.mutate()}
                  >
                    {credentialMutation.isPending ? "Binding…" : "Save binding"}
                  </Button>
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">
                  {boundCredential.data?.credential
                    ? `${boundCredential.data.credential.name} (${boundCredential.data.credential.host}:${boundCredential.data.credential.port})`
                    : "Anonymous access"}
                </p>
              )}
              <p className="text-xs text-muted-foreground">
                Only active credentials matching this repository&apos;s scheme, host, and port are
                available. Save URL changes before updating the binding.
              </p>
            </Field>
          </div>
        </SettingsCard>
      </form>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (canEdit) buildMutation.mutate();
        }}
      >
        <SettingsCard
          title="Build &amp; Output Settings"
          description="How the deployable output is produced inside the build container."
          hint="Leave fields empty to auto-detect from the framework."
          action={
            canEdit ? (
              <Button type="submit" size="sm" disabled={buildMutation.isPending}>
                {buildMutation.isPending ? "Saving…" : "Save"}
              </Button>
            ) : undefined
          }
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="settings-root">Root directory</FieldLabel>
              <Input
                id="settings-root"
                placeholder="."
                value={rootDirectory}
                readOnly={!canEdit}
                onChange={(event) => setRootDirectory(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-install">Install command</FieldLabel>
              <Input
                id="settings-install"
                placeholder="npm install"
                value={installCommand}
                readOnly={!canEdit}
                onChange={(event) => setInstallCommand(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-build">Build command</FieldLabel>
              <Input
                id="settings-build"
                placeholder="npm run build"
                value={buildCommand}
                readOnly={!canEdit}
                onChange={(event) => setBuildCommand(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-output">Output directory</FieldLabel>
              <Input
                id="settings-output"
                placeholder="dist"
                value={outputDirectory}
                readOnly={!canEdit}
                onChange={(event) => setOutputDirectory(event.target.value)}
              />
            </Field>
          </div>
        </SettingsCard>
      </form>
    </div>
  );
}

export function credentialMatchesRepository(
  credential: Pick<SourceCredential, "kind" | "host" | "port">,
  repositoryUrl: string | null,
) {
  if (!repositoryUrl) return false;
  const value = repositoryUrl.trim();
  if (!value.includes("://")) {
    const match = /^(?:[^@]+@)?(?:\[([^\]]+)\]|([^:]+)):(.+)$/.exec(value);
    const host = (match?.[1] ?? match?.[2])?.toLowerCase().replace(/\.$/, "");
    return credential.kind === "ssh" && credential.port === 22 && credential.host === host;
  }
  try {
    const parsed = new URL(value);
    const kind = parsed.protocol === "https:" ? "https" : parsed.protocol === "ssh:" ? "ssh" : null;
    if (!kind || credential.kind !== kind) return false;
    const port = parsed.port ? Number(parsed.port) : kind === "https" ? 443 : 22;
    return (
      credential.host === parsed.hostname.toLowerCase().replace(/\.$/, "") &&
      credential.port === port
    );
  } catch {
    return false;
  }
}
