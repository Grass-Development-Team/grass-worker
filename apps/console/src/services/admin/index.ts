import { announcementsApi } from "./announcements";
import { codesApi } from "./codes";
import { hostSourcesApi } from "./host-sources";
import { nodesApi } from "./nodes";
import { platformApi } from "./platform";
import { projectsApi } from "./projects";
import { providersApi } from "./providers";
import { quotaPlansApi } from "./quota-plans";
import { regionalIngressesApi } from "./regional-ingresses";
import { registrationApi } from "./registration";
import { reviewsApi } from "./reviews";
import { settingsApi } from "./settings";
import { storageApi } from "./storage";
import { teamGroupsApi } from "./team-groups";
import { teamsApi } from "./teams";
import { usersApi } from "./users";
export type * from "./model";
export const adminApi = {
  ...platformApi,
  ...codesApi,
  ...registrationApi,
  ...quotaPlansApi,
  ...hostSourcesApi,
  ...regionalIngressesApi,
  ...nodesApi,
  ...usersApi,
  ...teamsApi,
  ...teamGroupsApi,
  ...projectsApi,
  ...reviewsApi,
  ...settingsApi,
  ...storageApi,
  ...providersApi,
  ...announcementsApi,
};
