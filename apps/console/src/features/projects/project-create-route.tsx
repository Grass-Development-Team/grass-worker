import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, Code2Icon } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { useTeam } from "@/features/teams/team-context";
import { canCreateProject } from "@/features/teams/team-permissions";

import { projectsApi, type CreateProjectInput } from "./projects.api";

export const FRAMEWORK_PRESETS = [
  {
    id: "vite",
    label: "Vite (React, Vue, Svelte)",
    description: "Fast static sites with a modern frontend toolchain.",
    runtime: "static",
    install: "npm install",
    build: "npm run build",
    output: "dist",
  },
  {
    id: "nextjs",
    label: "Next.js (static export)",
    description: "Export a Next.js app as a static site.",
    runtime: "static",
    install: "npm install",
    build: "npm run build",
    output: "out",
  },
  {
    id: "nextjs-ssr",
    label: "Next.js (SSR)",
    description: "Run Next.js with server-side rendering.",
    runtime: "ssr",
    install: "npm install",
    build: "npm run build",
    output: "",
  },
  {
    id: "nuxt",
    label: "Nuxt (static)",
    description: "Generate a Nuxt app for static delivery.",
    runtime: "static",
    install: "npm install",
    build: "npm run generate",
    output: ".output/public",
  },
  {
    id: "nuxt-ssr",
    label: "Nuxt (SSR)",
    description: "Run Nuxt with server-side rendering.",
    runtime: "ssr",
    install: "npm install",
    build: "npm run build",
    output: "",
  },
  {
    id: "astro-ssr",
    label: "Astro (SSR)",
    description: "Deploy Astro with its Node adapter.",
    runtime: "ssr",
    install: "npm install",
    build: "npm run build",
    output: "",
  },
  {
    id: "sveltekit",
    label: "SvelteKit (static)",
    description: "Build SvelteKit with the static adapter.",
    runtime: "static",
    install: "npm install",
    build: "npm run build",
    output: "build",
  },
  {
    id: "astro",
    label: "Astro",
    description: "Ship a fast content-focused static site.",
    runtime: "static",
    install: "npm install",
    build: "npm run build",
    output: "dist",
  },
  {
    id: "cra",
    label: "Create React App",
    description: "Use the standard React build output.",
    runtime: "static",
    install: "npm install",
    build: "npm run build",
    output: "build",
  },
  {
    id: "custom",
    label: "Other / custom",
    description: "Start with a static project and configure it later.",
    runtime: "static",
    install: "",
    build: "",
    output: "",
  },
] as const;

type FrameworkPresetId = (typeof FRAMEWORK_PRESETS)[number]["id"];

function slugify(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function AccessState() {
  return (
    <div className="flex flex-1 items-center justify-center px-6 py-16">
      <div role="alert" className="max-w-sm space-y-4 text-center">
        <div>
          <h1 className="text-lg font-semibold">Project creation unavailable</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            You do not have permission to create projects in the active team.
          </p>
        </div>
        <Button variant="outline" asChild>
          <Link to="/projects">Back to projects</Link>
        </Button>
      </div>
    </div>
  );
}

export function ProjectCreateRoute() {
  const { activeTeam, activeRole } = useTeam();
  const teamId = activeTeam?.id;

  if (!teamId) {
    return (
      <div className="flex flex-1 items-center justify-center px-6 py-16">
        <p className="text-sm text-muted-foreground">Select a team to create a project.</p>
      </div>
    );
  }

  if (!activeRole || !canCreateProject(activeRole)) {
    return <AccessState />;
  }

  return <CreateProjectForm teamId={teamId} />;
}

function CreateProjectForm({ teamId }: { teamId: string }) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [preset, setPreset] = useState<FrameworkPresetId>("vite");
  const [repositoryUrl, setRepositoryUrl] = useState("");
  const [defaultBranch, setDefaultBranch] = useState("main");

  const createMutation = useMutation({
    mutationFn: (input: CreateProjectInput) => projectsApi.create(input),
    onSuccess: async ({ project }) => {
      await queryClient.invalidateQueries({ queryKey: ["projects", teamId] });
      navigate(`/projects/${project.id}`);
    },
  });

  const selectedPreset =
    FRAMEWORK_PRESETS.find((entry) => entry.id === preset) ?? FRAMEWORK_PRESETS[0];

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createMutation.mutate({
      team_id: teamId,
      name,
      slug,
      runtime: selectedPreset.runtime,
      repository_url: repositoryUrl || undefined,
      default_branch: defaultBranch || undefined,
      install_command: selectedPreset.install || undefined,
      build_command: selectedPreset.build || undefined,
      output_directory: selectedPreset.output || undefined,
      framework_hint: preset !== "custom" ? preset : undefined,
    });
  };

  return (
    <div className="mx-auto grid min-h-[calc(100svh-3.5rem)] w-full max-w-[1440px] flex-1 lg:grid-cols-2">
      <section className="flex flex-col border-b p-6 sm:p-10 lg:border-b-0 lg:border-r">
        <div className="max-w-xl">
          <p className="text-sm font-medium text-muted-foreground">Start with the basics</p>
          <h1 className="mt-3 text-3xl font-semibold tracking-tight sm:text-4xl">
            Configure project
          </h1>
          <p className="mt-3 max-w-lg text-sm leading-6 text-muted-foreground">
            Connect a repository and choose a framework preset. You can refine the build settings
            after the project is created.
          </p>
        </div>

        <form onSubmit={submit} className="mt-10 flex max-w-xl flex-1 flex-col">
          <div className="space-y-6">
            <Field>
              <FieldLabel htmlFor="project-name">Project name</FieldLabel>
              <Input
                id="project-name"
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                  if (!slugTouched) setSlug(slugify(event.target.value));
                }}
                autoFocus
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
              <FieldLabel htmlFor="project-repo">Git repository URL</FieldLabel>
              <Input
                id="project-repo"
                type="url"
                placeholder="https://github.com/acme/site.git"
                value={repositoryUrl}
                onChange={(event) => setRepositoryUrl(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="project-branch">Default branch</FieldLabel>
              <Input
                id="project-branch"
                value={defaultBranch}
                onChange={(event) => setDefaultBranch(event.target.value)}
              />
            </Field>
          </div>

          <div className="mt-auto pt-10">
            {createMutation.isError && (
              <p role="alert" className="mb-4 text-sm text-destructive">
                {createMutation.error instanceof Error
                  ? createMutation.error.message
                  : "Unable to create the project."}
              </p>
            )}
            <Button type="submit" size="lg" className="w-full" disabled={createMutation.isPending}>
              {createMutation.isPending ? "Creating project..." : "Create project"}
            </Button>
          </div>
        </form>
      </section>

      <section className="flex flex-col p-6 sm:p-10">
        <div>
          <p className="text-sm font-medium text-muted-foreground">Choose your stack</p>
          <h2 className="mt-3 text-3xl font-semibold tracking-tight sm:text-4xl">
            Framework Presets
          </h2>
          <p className="mt-3 max-w-xl text-sm leading-6 text-muted-foreground">
            Start from a known build setup, or choose custom and configure it later.
          </p>
        </div>

        <div className="mt-10 grid flex-1 content-start grid-cols-1 gap-3 sm:grid-cols-2">
          {FRAMEWORK_PRESETS.map((entry) => {
            const selected = entry.id === preset;
            return (
              <button
                key={entry.id}
                type="button"
                aria-pressed={selected}
                className={`group flex min-h-32 flex-col justify-between rounded-lg border p-4 text-left transition-colors focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] ${
                  selected
                    ? "border-primary bg-primary/5 dark:bg-primary/10"
                    : "border-border bg-card hover:border-foreground/30 hover:bg-accent/50"
                }`}
                onClick={() => setPreset(entry.id)}
              >
                <span className="flex items-start justify-between gap-4">
                  <span className="flex size-9 items-center justify-center rounded-md border bg-background text-muted-foreground">
                    <Code2Icon />
                  </span>
                  {selected && <CheckIcon className="size-4 text-primary" aria-hidden="true" />}
                </span>
                <span className="mt-6">
                  <span className="block text-sm font-semibold text-foreground">{entry.label}</span>
                  <span className="mt-1 block text-xs leading-5 text-muted-foreground">
                    {entry.description}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      </section>
    </div>
  );
}
