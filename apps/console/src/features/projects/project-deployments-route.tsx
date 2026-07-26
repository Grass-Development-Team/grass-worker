import { DeploymentsTab } from "@/features/deployments/deployments-tab";

import { useProject } from "./project-layout";

export function ProjectDeploymentsRoute() {
  const { project } = useProject();

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-lg font-semibold">Deployments</h1>
        <p className="text-sm text-muted-foreground">
          Every build of {project.name}, newest first.
        </p>
      </div>
      <DeploymentsTab projectId={project.id} />
    </div>
  );
}
