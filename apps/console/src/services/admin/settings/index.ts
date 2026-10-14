import { request } from "@/infra/api/index";
import type { AdminMfaPolicy, AdminPasswordPolicy, AdminSettings } from "./model";
export const settingsApi = {
  getSettings: () => request<AdminSettings>("/api/v1/admin/settings"),

  updateSettings: (input: {
    site_name?: string;
    site_logo_url?: string;
    site_url?: string;
    public_base_url?: string;
    signup_policy?: "open" | "invite_only" | "closed";
    review_production?: "auto" | "manual";
    review_preview?: "auto" | "manual";
    domain_review_default?: "auto" | "manual";
    server_host?: string;
    server_port?: number;
    redis_backend?: "moka" | "redis";
    session_cookie_secure?: boolean;
    session_idle_ttl_seconds?: number;
    session_ttl_seconds?: number;
    audit_retention_days?: number;
    mail_mode?: "none" | "local" | "smtp";
    mail_from_address?: string;
    mail_from_name?: string;
    mail_sendmail_command?: string;
    mail_smtp_host?: string;
    mail_smtp_port?: number;
    mail_smtp_security?: "none" | "starttls" | "tls";
    mail_smtp_username?: string;
    mail_smtp_password?: string;
    password_policy?: AdminPasswordPolicy;
    registration_email_verification?: boolean;
    mfa_policy?: AdminMfaPolicy;
    node_manager_auto_start_local_node?: boolean;
    node_manager_local_node_binary?: string;
    node_manager_local_node_config?: string;
    node_manager_restart_on_exit?: boolean;
    migration_auto_migrate?: boolean;
    log_level?: string;
    log_format?: "pretty" | "json";
  }) =>
    request<AdminSettings>("/api/v1/admin/settings", {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
};
