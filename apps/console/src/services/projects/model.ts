export type ProjectRuntime = "static" | "ssr" | "hybrid" | "serverless" | "edge";

export type HostStatus = "pending" | "active" | "failed" | "disabled";

export type HostEnvironment = "production" | "preview" | "all";

export interface Project {
  id: string;
  team_id: string;
  slug: string;
  name: string;
  runtime: ProjectRuntime;
  repository_url: string | null;
  default_branch: string | null;
  install_command: string | null;
  build_command: string | null;
  output_directory: string | null;
  source_config: { root_directory?: string | null; framework_hint?: string | null };
  build_config: Record<string, unknown>;
  archived_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface HostAssignment {
  assigned: boolean;
  host?: string;
  status?: HostStatus;
  failure_reason?: string | null;
  reason?: string;
}

export interface ProvisionEvent {
  id: string;
  status: "success" | "pending" | "failed";
  operation: string;
  error_message: string | null;
  created_at: string;
}

export interface ProjectHost {
  id: string;
  project_id: string;
  host: string;
  region?: string;
  kind: "platform" | "custom";
  environment: HostEnvironment;
  status: HostStatus;
  failure_reason: string | null;
  is_primary: boolean;
  host_source_id: string | null;
  serving?: boolean;
  created_at: string;
  provision_events?: ProvisionEvent[];
  ingress?: ProjectHostIngress | null;
  ownership_status?: "pending" | "verified" | "failed" | "not_required";
  ownership_checked_at?: string | null;
  ownership_error?: string | null;
  certificate?: DomainCertificate | null;
  connection_state?: string;
  onboarding?: {
    dns_status: string;
    dns_error: string | null;
    checked_at: string | null;
    next_check_at: string;
  } | null;
}

export interface DomainCertificate {
  status: "pending" | "issuing" | "active" | "expiring" | "failed" | "disabled";
  issuer: "letsencrypt" | "zerossl" | "manual";
  platform_issuer: "letsencrypt" | "zerossl";
  // Installation progress is included in domain list and connection-check responses.
  https_ready?: boolean;
  installed_nodes?: number;
  required_nodes?: number;
  challenge_method: "http01";
  auto_renew: boolean;
  issued_at: string | null;
  expires_at: string | null;
  error: string | null;
  retry_at: string | null;
  revision: string | null;
}

export interface ProjectHostIngress {
  region: string;
  cname: { record_type: "CNAME"; name: string; target: string };
  txt: { record_type: "TXT"; name: string; value: string };
  origin_host_preservation: boolean;
  health_check: { path: string; interval_seconds: number };
  entrance_nodes: Array<{ node_id: string; base_url: string; priority: number }>;
  certificate: {
    enabled: boolean;
    issuer: "letsencrypt" | "zerossl" | "manual";
    auto_renew: boolean;
    status: "pending" | "issuing" | "active" | "expiring" | "failed" | "disabled";
    expires_at: string | null;
    error: string | null;
  };
}

export interface CreateProjectInput {
  team_id: string;
  name: string;
  slug: string;
  runtime: "static" | "ssr";
  repository_url?: string;
  default_branch?: string;
  root_directory?: string;
  install_command?: string;
  build_command?: string;
  output_directory?: string;
  framework_hint?: string;
}

export interface UpdateProjectInput {
  name?: string;
  repository_url?: string;
  default_branch?: string;
  install_command?: string;
  build_command?: string;
  output_directory?: string;
  root_directory?: string;
  framework_hint?: string;
}

export interface BoundSourceCredential {
  id: string;
  name: string;
  kind: "https" | "ssh";
  host: string;
  port: number;
  username: string | null;
  revoked: boolean;
}
