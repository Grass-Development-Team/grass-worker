import { DeploymentsTab } from "@/features/deployments/deployments-tab";
import { canContributeToProjects } from "@/features/teams/team-permissions";

import { useProject } from "./project-layout";

export function ProjectDeploymentsRoute() {
  const { project, role } = useProject();

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-lg font-semibold">Deployments</h1>
        <p className="text-sm text-muted-foreground">
          Every build of {project.name}, newest first.
        </p>
      </div>
      <DeploymentsTab
        projectId={project.id}
        canDeploy={canContributeToProjects(role) && Boolean(project.repository_url)}
        hasRepository={Boolean(project.repository_url)}
      />
    </div>
  );
}
