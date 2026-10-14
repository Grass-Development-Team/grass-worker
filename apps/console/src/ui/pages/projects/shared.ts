import { QueryController } from "@/infra/query";
import { projectsApi } from "@/services/projects";
import type { TeamRole } from "@/services/teams";
import {
  canContributeToProjects,
  canManageMembers,
  canManageProjectLifecycle,
} from "@/services/teams/permissions";
import type { PropertyValues } from "lit";
import { Page } from "../base";
export class ProjectPage extends Page {
  protected project = new QueryController(this, {
    queryKey: ["project", "none"],
    queryFn: () => projectsApi.get(this.projectId),
    enabled: false,
  });
  protected get projectId() {
    return this.context.params.projectId || "";
  }
  protected get canEdit() {
    return Boolean(
      this.project.result.data &&
      canContributeToProjects(this.project.result.data.role as TeamRole),
    );
  }
  protected get canManage() {
    return Boolean(
      this.project.result.data &&
      canManageProjectLifecycle(this.project.result.data.role as TeamRole),
    );
  }
  protected get canManageCredentials() {
    return Boolean(
      this.project.result.data && canManageMembers(this.project.result.data.role as TeamRole),
    );
  }
  protected willUpdate(changed: PropertyValues): void {
    if (changed.has("context"))
      this.project.configure({
        queryKey: ["project", this.projectId],
        queryFn: () => projectsApi.get(this.projectId),
        enabled: Boolean(this.projectId),
      });
  }
}
