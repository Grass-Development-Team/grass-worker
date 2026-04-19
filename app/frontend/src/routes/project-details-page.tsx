import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams } from "react-router-dom";
import { getProject, projectQueryKey } from "@/api/projects";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

function formatTimestamp(value: string) {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

export function ProjectDetailsPage() {
  const navigate = useNavigate();
  const { projectId } = useParams<{ projectId: string }>();
  const query = useQuery({
    queryKey: projectQueryKey(projectId ?? ""),
    queryFn: () => getProject(projectId ?? ""),
    enabled: Boolean(projectId),
  });

  if (!projectId) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-muted/30 p-6">
        <Card className="w-full max-w-lg">
          <CardHeader>
            <CardTitle>Project not found</CardTitle>
            <CardDescription>The requested project identifier is missing.</CardDescription>
          </CardHeader>
        </Card>
      </main>
    );
  }

  return (
    <main className="min-h-screen bg-muted/30 px-6 py-10">
      <div className="mx-auto max-w-5xl space-y-6">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <Button onClick={() => void navigate("/projects")} type="button" variant="outline">
            Back to projects
          </Button>
          <Button
            disabled={query.isPending}
            onClick={() => void query.refetch()}
            type="button"
            variant="outline"
          >
            {query.isPending ? "Refreshing..." : "Refresh details"}
          </Button>
        </div>

        {query.isPending ? (
          <Card>
            <CardHeader>
              <CardTitle>Loading project</CardTitle>
              <CardDescription>Fetching project metadata from the control API.</CardDescription>
            </CardHeader>
          </Card>
        ) : query.isError ? (
          <Alert variant="destructive">
            <AlertTitle>Unable to load project</AlertTitle>
            <AlertDescription>
              {query.error instanceof Error ? query.error.message : "Project lookup failed."}
            </AlertDescription>
          </Alert>
        ) : (
          <>
            <div className="space-y-1">
              <p className="text-sm text-muted-foreground">{query.data.slug}</p>
              <h1 className="text-3xl font-semibold tracking-tight">{query.data.name}</h1>
              <p className="text-sm text-muted-foreground">
                {query.data.status === "active" ? "Active project" : "Archived project"}
              </p>
            </div>

            <Card>
              <CardHeader>
                <CardTitle>Overview</CardTitle>
                <CardDescription>Core metadata for the selected project.</CardDescription>
              </CardHeader>
              <CardContent className="grid gap-4 text-sm text-muted-foreground sm:grid-cols-3">
                <div className="space-y-1">
                  <p>Slug</p>
                  <p className="font-medium text-foreground">{query.data.slug}</p>
                </div>
                <div className="space-y-1">
                  <p>Status</p>
                  <p className="font-medium text-foreground">
                    {query.data.status === "active" ? "Active" : "Archived"}
                  </p>
                </div>
                <div className="space-y-1">
                  <p>Created</p>
                  <p className="font-medium text-foreground">
                    {formatTimestamp(query.data.created_at)}
                  </p>
                </div>
                <div className="space-y-1 sm:col-span-3">
                  <p>{query.data.archived_at ? "Archived" : "Updated"}</p>
                  <p className="font-medium text-foreground">
                    {formatTimestamp(query.data.archived_at ?? query.data.updated_at)}
                  </p>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle>Deployment history</CardTitle>
                <CardDescription>
                  This route is ready for deployment records to plug in next.
                </CardDescription>
              </CardHeader>
              <CardContent className="text-sm text-muted-foreground">
                No deployments have been loaded for this project yet.
              </CardContent>
            </Card>
          </>
        )}
      </div>
    </main>
  );
}
