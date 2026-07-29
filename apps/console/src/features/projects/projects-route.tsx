import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderGitIcon, PlusIcon } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate } from "react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useTeam } from "@/features/teams/team-context";
import { canCreateProject } from "@/features/teams/team-permissions";

import { projectsApi, type CreateProjectInput } from "./projects.api";

const FRAMEWORK_PRESETS = [
  {
    id: "vite",
    label: "Vite (React, Vue, Svelte…)",
    install: "npm install",
    build: "npm run build",
    output: "dist",
  },
  {
    id: "nextjs",
    label: "Next.js (static export)",
    install: "npm install",
    build: "npm run build",
    output: "out",
  },
  {
    id: "nextjs-ssr",
    label: "Next.js (SSR)",
    install: "npm install",
    build: "npm run build",
    output: "",
  },
  {
    id: "nuxt",
    label: "Nuxt (static)",
    install: "npm install",
    build: "npm run generate",
    output: ".output/public",
  },
  {
    id: "nuxt-ssr",
    label: "Nuxt (SSR)",
    install: "npm install",
    build: "npm run build",
    output: "",
  },
  {
    id: "astro-ssr",
    label: "Astro (SSR, node adapter)",
    install: "npm install",
    build: "npm run build",
    output: "",
  },
  {
    id: "sveltekit",
    label: "SvelteKit (adapter-static)",
    install: "npm install",
    build: "npm run build",
    output: "build",
  },
  { id: "astro", label: "Astro", install: "npm install", build: "npm run build", output: "dist" },
  {
    id: "cra",
    label: "Create React App",
    install: "npm install",
    build: "npm run build",
    output: "build",
  },
  { id: "custom", label: "Other / custom", install: "", build: "", output: "" },
] as const;

type FrameworkPresetId = (typeof FRAMEWORK_PRESETS)[number]["id"];

function slugify(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function ProjectsRoute() {
  const { activeTeam, activeRole } = useTeam();
  const teamId = activeTeam?.id;
  const showCreateProject = Boolean(activeRole && canCreateProject(activeRole));

  const projectsQuery = useQuery({
    queryKey: ["projects", teamId],
    queryFn: () => projectsApi.list(teamId as string),
    enabled: Boolean(teamId),
  });

  if (!teamId) {
    return <p className="text-sm text-muted-foreground">Select a team to view its projects.</p>;
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold">Projects</h1>
          <p className="text-sm text-muted-foreground">
            Deployable projects owned by {activeTeam?.name}.
          </p>
        </div>
        {showCreateProject && <CreateProjectDialog teamId={teamId} />}
      </div>

      {projectsQuery.isLoading && <Skeleton className="h-64 w-full" aria-busy="true" />}
      {projectsQuery.isError && (
        <p role="alert" className="text-sm text-destructive">
          {projectsQuery.error instanceof Error
            ? projectsQuery.error.message
            : "Unable to load projects."}
        </p>
      )}
      {projectsQuery.data &&
        (projectsQuery.data.projects.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <FolderGitIcon />
              </EmptyMedia>
              <EmptyTitle>No projects yet</EmptyTitle>
              <EmptyDescription>
                {showCreateProject
                  ? "Create your first project to start deploying static sites."
                  : "No projects are available for this team."}
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Runtime</TableHead>
                <TableHead>Repository</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Created</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {projectsQuery.data.projects.map((project) => (
                <TableRow key={project.id}>
                  <TableCell>
                    <Link to={`/projects/${project.id}`} className="font-medium hover:underline">
                      {project.name}
                    </Link>
                    <p className="text-xs text-muted-foreground">{project.slug}</p>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline">{project.runtime}</Badge>
                  </TableCell>
                  <TableCell className="max-w-56 truncate text-sm text-muted-foreground">
                    {project.repository_url ?? "—"}
                  </TableCell>
                  <TableCell>
                    {project.archived_at ? (
                      <Badge variant="secondary">Archived</Badge>
                    ) : (
                      <Badge variant="success">Active</Badge>
                    )}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(project.created_at).toLocaleDateString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ))}
    </div>
  );
}

function CreateProjectDialog({ teamId }: { teamId: string }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [runtime, setRuntime] = useState<"static" | "ssr">("static");
  const [preset, setPreset] = useState<FrameworkPresetId>("vite");
  const [repositoryUrl, setRepositoryUrl] = useState("");
  const [defaultBranch, setDefaultBranch] = useState("main");
  const [installCommand, setInstallCommand] = useState("");
  const [buildCommand, setBuildCommand] = useState("");
  const [outputDirectory, setOutputDirectory] = useState("");
  const [assignmentNote, setAssignmentNote] = useState<string | null>(null);

  const createMutation = useMutation({
    mutationFn: (input: CreateProjectInput) => projectsApi.create(input),
    onSuccess: async ({ project, host_assignment }) => {
      await queryClient.invalidateQueries({ queryKey: ["projects", teamId] });
      if (!host_assignment.assigned && host_assignment.reason) {
        setAssignmentNote(host_assignment.reason);
      }
      setOpen(false);
      navigate(`/projects/${project.id}`);
    },
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    createMutation.mutate({
      team_id: teamId,
      name,
      slug,
      runtime,
      repository_url: repositoryUrl || undefined,
      default_branch: defaultBranch || undefined,
      install_command: installCommand || undefined,
      build_command: buildCommand || undefined,
      output_directory: outputDirectory || undefined,
      framework_hint: preset !== "custom" ? preset : undefined,
    });
  };

  return (
    <>
      {assignmentNote && (
        <p className="text-xs text-muted-foreground">Host assignment skipped: {assignmentNote}</p>
      )}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogTrigger asChild>
          <Button>
            <PlusIcon /> New project
          </Button>
        </DialogTrigger>
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Create project</DialogTitle>
            <DialogDescription>
              Configure the source repository and build settings. A platform domain is assigned
              automatically when a host source is available.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={submit} className="space-y-4">
            <Field>
              <FieldLabel htmlFor="project-name">Name</FieldLabel>
              <Input
                id="project-name"
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                  if (!slugTouched) setSlug(slugify(event.target.value));
                }}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="project-slug">Slug</FieldLabel>
              <Input
                id="project-slug"
                value={slug}
                onChange={(event) => {
                  setSlugTouched(true);
                  setSlug(event.target.value);
                }}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="project-runtime">Runtime</FieldLabel>
              <Select value={runtime} onValueChange={(value) => setRuntime(value as never)}>
                <SelectTrigger id="project-runtime">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="static">Static</SelectItem>
                  <SelectItem value="ssr">SSR (Astro, Next.js, Nuxt)</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="project-repo">Git repository URL</FieldLabel>
              <Input
                id="project-repo"
                placeholder="https://github.com/acme/site.git"
                value={repositoryUrl}
                onChange={(event) => setRepositoryUrl(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="project-preset">Framework preset</FieldLabel>
              <Select
                value={preset}
                onValueChange={(value) => {
                  const id = value as FrameworkPresetId;
                  setPreset(id);
                  const found = FRAMEWORK_PRESETS.find((entry) => entry.id === id);
                  if (found && found.id !== "custom") {
                    setInstallCommand(found.install);
                    setBuildCommand(found.build);
                    setOutputDirectory(found.output);
                  }
                }}
              >
                <SelectTrigger id="project-preset">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {FRAMEWORK_PRESETS.map((entry) => (
                    <SelectItem key={entry.id} value={entry.id}>
                      {entry.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <div className="grid grid-cols-2 gap-4">
              <Field>
                <FieldLabel htmlFor="project-branch">Default branch</FieldLabel>
                <Input
                  id="project-branch"
                  value={defaultBranch}
                  onChange={(event) => setDefaultBranch(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="project-output">Output directory</FieldLabel>
                <Input
                  id="project-output"
                  placeholder="dist"
                  value={outputDirectory}
                  onChange={(event) => setOutputDirectory(event.target.value)}
                />
              </Field>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <Field>
                <FieldLabel htmlFor="project-install">Install command</FieldLabel>
                <Input
                  id="project-install"
                  placeholder="npm install"
                  value={installCommand}
                  onChange={(event) => setInstallCommand(event.target.value)}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="project-build">Build command</FieldLabel>
                <Input
                  id="project-build"
                  placeholder="npm run build"
                  value={buildCommand}
                  onChange={(event) => setBuildCommand(event.target.value)}
                />
              </Field>
            </div>
            {createMutation.isError && (
              <p role="alert" className="text-sm text-destructive">
                {createMutation.error instanceof Error
                  ? createMutation.error.message
                  : "Unable to create the project."}
              </p>
            )}
            <DialogFooter>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending ? "Creating…" : "Create project"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
