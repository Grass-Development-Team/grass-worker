import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArchiveIcon, ArchiveRestoreIcon, GlobeIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";

import { Badge } from "@/components/ui/badge";
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
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { DeploymentsTab } from "@/features/deployments/deployments-tab";

import { projectsApi, type HostStatus, type Project } from "./projects.api";

export function hostStatusVariant(
  status: HostStatus,
): "success" | "warning" | "destructive" | "secondary" {
  switch (status) {
    case "active":
      return "success";
    case "pending":
      return "warning";
    case "failed":
      return "destructive";
    case "disabled":
      return "secondary";
  }
}

export function ProjectDetailRoute() {
  const { projectId } = useParams<{ projectId: string }>();

  const projectQuery = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => projectsApi.get(projectId as string),
    enabled: Boolean(projectId),
  });

  if (projectQuery.isLoading) {
    return <Skeleton className="h-96 w-full" aria-busy="true" />;
  }
  if (projectQuery.isError || !projectQuery.data) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {projectQuery.error instanceof Error
          ? projectQuery.error.message
          : "Unable to load this project."}
      </p>
    );
  }

  const { project } = projectQuery.data;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-lg font-semibold">{project.name}</h1>
            <Badge variant="outline">{project.runtime}</Badge>
            {project.archived_at && <Badge variant="secondary">Archived</Badge>}
          </div>
          <p className="text-sm text-muted-foreground">
            {project.slug}
            {project.repository_url ? ` · ${project.repository_url}` : ""}
          </p>
        </div>
        <Button variant="outline" asChild>
          <Link to="/projects">Back to projects</Link>
        </Button>
      </div>

      <Tabs defaultValue="deployments">
        <TabsList>
          <TabsTrigger value="deployments">Deployments</TabsTrigger>
          <TabsTrigger value="hosts">Hosts</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
        </TabsList>
        <TabsContent value="deployments">
          <DeploymentsTab projectId={project.id} />
        </TabsContent>
        <TabsContent value="hosts">
          <HostsTab projectId={project.id} />
        </TabsContent>
        <TabsContent value="settings">
          <SettingsTab project={project} />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function HostsTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [newHost, setNewHost] = useState("");
  const [error, setError] = useState<string | null>(null);

  const hostsQuery = useQuery({
    queryKey: ["project-hosts", projectId],
    queryFn: () => projectsApi.listHosts(projectId),
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["project-hosts", projectId] });

  const addMutation = useMutation({
    mutationFn: () => projectsApi.createHost(projectId, { host: newHost }),
    onSuccess: () => {
      setNewHost("");
      setError(null);
      invalidate();
    },
    onError: (cause) => setError(cause instanceof Error ? cause.message : "Unable to add host."),
  });
  const removeMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.removeHost(projectId, hostId),
    onSuccess: invalidate,
    onError: (cause) => setError(cause instanceof Error ? cause.message : "Unable to remove host."),
  });
  const primaryMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.setPrimaryHost(projectId, hostId),
    onSuccess: invalidate,
    onError: (cause) => setError(cause instanceof Error ? cause.message : "Unable to set primary."),
  });
  const provisionMutation = useMutation({
    mutationFn: (hostId: string) => projectsApi.provisionHost(projectId, hostId),
    onSuccess: invalidate,
    onError: (cause) =>
      setError(cause instanceof Error ? cause.message : "Unable to retry provisioning."),
  });

  return (
    <div className="space-y-4">
      <form
        className="flex items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          if (newHost.trim()) addMutation.mutate();
        }}
      >
        <Field className="max-w-sm flex-1">
          <FieldLabel htmlFor="new-host">Add custom host</FieldLabel>
          <Input
            id="new-host"
            placeholder="app.example.com"
            value={newHost}
            onChange={(event) => setNewHost(event.target.value)}
          />
        </Field>
        <Button type="submit" disabled={addMutation.isPending || !newHost.trim()}>
          <PlusIcon /> Add host
        </Button>
      </form>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      {hostsQuery.isLoading && <Skeleton className="h-40 w-full" aria-busy="true" />}
      {hostsQuery.data &&
        (hostsQuery.data.hosts.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <GlobeIcon className="mr-1 inline size-4" />
            No hosts bound yet. Platform domains are assigned automatically when a host source is
            configured.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Host</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>Environment</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {hostsQuery.data.hosts.map((host) => (
                <TableRow key={host.id}>
                  <TableCell>
                    <span className="font-medium">{host.host}</span>
                    {host.is_primary && (
                      <Badge variant="outline" className="ml-2">
                        Primary
                      </Badge>
                    )}
                    {host.failure_reason && (
                      <p className="text-xs text-destructive">{host.failure_reason}</p>
                    )}
                  </TableCell>
                  <TableCell className="capitalize">{host.kind}</TableCell>
                  <TableCell className="capitalize">{host.environment}</TableCell>
                  <TableCell>
                    <Badge variant={hostStatusVariant(host.status)}>{host.status}</Badge>
                  </TableCell>
                  <TableCell className="space-x-1 text-right">
                    {(host.status === "pending" || host.status === "failed") &&
                      host.host_source_id && (
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => provisionMutation.mutate(host.id)}
                          disabled={provisionMutation.isPending}
                        >
                          Retry
                        </Button>
                      )}
                    {!host.is_primary && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => primaryMutation.mutate(host.id)}
                        disabled={primaryMutation.isPending}
                      >
                        Make primary
                      </Button>
                    )}
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Remove ${host.host}`}
                      onClick={() => removeMutation.mutate(host.id)}
                      disabled={removeMutation.isPending}
                    >
                      <Trash2Icon />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}

function SettingsTab({ project }: { project: Project }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState(project.name);
  const [repositoryUrl, setRepositoryUrl] = useState(project.repository_url ?? "");
  const [defaultBranch, setDefaultBranch] = useState(project.default_branch ?? "");
  const [rootDirectory, setRootDirectory] = useState(project.source_config.root_directory ?? "");
  const [installCommand, setInstallCommand] = useState(project.install_command ?? "");
  const [buildCommand, setBuildCommand] = useState(project.build_command ?? "");
  const [outputDirectory, setOutputDirectory] = useState(project.output_directory ?? "");
  const [message, setMessage] = useState<string | null>(null);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["project", project.id] });

  const updateMutation = useMutation({
    mutationFn: () =>
      projectsApi.update(project.id, {
        name,
        repository_url: repositoryUrl,
        default_branch: defaultBranch,
        root_directory: rootDirectory,
        install_command: installCommand,
        build_command: buildCommand,
        output_directory: outputDirectory,
      }),
    onSuccess: () => {
      setMessage("Project settings saved.");
      invalidate();
    },
    onError: (cause) =>
      setMessage(cause instanceof Error ? cause.message : "Unable to save settings."),
  });

  const archiveMutation = useMutation({
    mutationFn: () =>
      project.archived_at ? projectsApi.unarchive(project.id) : projectsApi.archive(project.id),
    onSuccess: invalidate,
    onError: (cause) =>
      setMessage(cause instanceof Error ? cause.message : "Unable to change archive state."),
  });

  const deleteMutation = useMutation({
    mutationFn: () => projectsApi.softDelete(project.id),
    onSuccess: () => navigate("/projects"),
    onError: (cause) =>
      setMessage(cause instanceof Error ? cause.message : "Unable to delete the project."),
  });

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Source &amp; build</CardTitle>
          <CardDescription>
            Where the source lives and how the static output is produced.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              updateMutation.mutate();
            }}
          >
            <Field>
              <FieldLabel htmlFor="settings-name">Project name</FieldLabel>
              <Input
                id="settings-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="settings-repo">Git repository URL</FieldLabel>
              <Input
                id="settings-repo"
                value={repositoryUrl}
                onChange={(event) => setRepositoryUrl(event.target.value)}
              />
            </Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="settings-branch">Default branch</FieldLabel>
                <Input
                  id="settings-branch"
                  value={defaultBranch}
                  onChange={(event) => setDefaultBranch(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-root">Root directory</FieldLabel>
                <Input
                  id="settings-root"
                  placeholder="."
                  value={rootDirectory}
                  onChange={(event) => setRootDirectory(event.target.value)}
                />
              </Field>
            </div>
            <div className="grid gap-4 sm:grid-cols-3">
              <Field>
                <FieldLabel htmlFor="settings-install">Install command</FieldLabel>
                <Input
                  id="settings-install"
                  value={installCommand}
                  onChange={(event) => setInstallCommand(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-build">Build command</FieldLabel>
                <Input
                  id="settings-build"
                  value={buildCommand}
                  onChange={(event) => setBuildCommand(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-output">Output directory</FieldLabel>
                <Input
                  id="settings-output"
                  value={outputDirectory}
                  onChange={(event) => setOutputDirectory(event.target.value)}
                />
              </Field>
            </div>
            {message && <p className="text-sm text-muted-foreground">{message}</p>}
            <Button type="submit" disabled={updateMutation.isPending}>
              {updateMutation.isPending ? "Saving…" : "Save settings"}
            </Button>
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Danger zone</CardTitle>
          <CardDescription>Archive pauses deployments; delete hides the project.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={() => archiveMutation.mutate()}
            disabled={archiveMutation.isPending}
          >
            {project.archived_at ? (
              <>
                <ArchiveRestoreIcon /> Unarchive
              </>
            ) : (
              <>
                <ArchiveIcon /> Archive
              </>
            )}
          </Button>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="destructive" disabled={deleteMutation.isPending}>
                <Trash2Icon /> Delete project
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Delete this project?</AlertDialogTitle>
                <AlertDialogDescription>
                  The project is soft-deleted and can be restored by an administrator. Active
                  deployments stop being served.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  className={buttonVariants({ variant: "destructive" })}
                  onClick={() => deleteMutation.mutate()}
                >
                  Delete project
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardContent>
      </Card>
    </div>
  );
}
