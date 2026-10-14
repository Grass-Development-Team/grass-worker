import type { AdminSettings } from "@/services/admin";
import type { FieldSpec } from "../../base";
const options = (values: string[]) =>
  values.map((value) => ({ value, label: value.replaceAll("_", " ") }));
export function settingsFields(section: string): FieldSpec[] {
  if (section === "basic")
    return [
      { name: "site_name", label: "Site name", required: true },
      {
        name: "site_logo_url",
        label: "Logo URL",
        description: "An HTTPS URL or a path served by this site.",
      },
      { name: "site_url", label: "Console URL", type: "url", required: true },
      { name: "public_base_url", label: "Public base URL", type: "url", required: true },
    ];
  if (section === "email")
    return [
      { name: "mail_mode", label: "Delivery mode", options: options(["none", "local", "smtp"]) },
      { name: "mail_from_address", label: "From address", type: "email" },
      { name: "mail_from_name", label: "From name" },
      { name: "mail_sendmail_command", label: "Sendmail command" },
      { name: "mail_smtp_host", label: "SMTP host" },
      { name: "mail_smtp_port", label: "SMTP port", type: "number", min: "1", max: "65535" },
      {
        name: "mail_smtp_security",
        label: "SMTP security",
        options: options(["none", "starttls", "tls"]),
      },
      { name: "mail_smtp_username", label: "SMTP username" },
      {
        name: "mail_smtp_password",
        label: "SMTP password",
        type: "password",
        description: "Leave blank to preserve the saved password.",
      },
    ];
  if (section === "governance")
    return [
      {
        name: "signup_policy",
        label: "Signup policy",
        options: options(["open", "invite_only", "closed"]),
      },
      {
        name: "review_production",
        label: "Production review",
        options: options(["auto", "manual"]),
      },
      { name: "review_preview", label: "Preview review", options: options(["auto", "manual"]) },
      {
        name: "domain_review_default",
        label: "Custom domain review",
        options: options(["auto", "manual"]),
      },
      { name: "session_cookie_secure", label: "Secure session cookies", type: "boolean" },
      {
        name: "session_idle_ttl_seconds",
        label: "Session idle TTL (seconds)",
        type: "number",
        min: "1",
        required: true,
      },
      {
        name: "session_ttl_seconds",
        label: "Absolute session TTL (seconds)",
        type: "number",
        min: "1",
        required: true,
      },
      {
        name: "audit_retention_days",
        label: "Audit retention (days)",
        type: "number",
        min: "0",
        description: "0 keeps events permanently.",
      },
    ];
  if (section === "runtime")
    return [
      {
        name: "node_manager_auto_start_local_node",
        label: "Auto-start local Node",
        type: "boolean",
      },
      { name: "node_manager_local_node_binary", label: "Local Node binary", required: true },
      { name: "node_manager_local_node_config", label: "Local Node config", required: true },
      { name: "node_manager_restart_on_exit", label: "Restart Node on exit", type: "boolean" },
      { name: "migration_auto_migrate", label: "Migrate database on startup", type: "boolean" },
      { name: "log_level", label: "Log filter", required: true },
      { name: "log_format", label: "Log format", options: options(["pretty", "json"]) },
    ];
  return [];
}
export function settingsValues(value: AdminSettings): Record<string, unknown> {
  return {
    site_name: value.site.name,
    site_logo_url: value.site.logo_url,
    site_url: value.site.url,
    public_base_url: value.site.public_base_url,
    signup_policy: value.signup.policy,
    review_production: value.review.production,
    review_preview: value.review.preview,
    domain_review_default: value.domain_review.default,
    mail_mode: value.mail.mode,
    mail_from_address: value.mail.from_address,
    mail_from_name: value.mail.from_name,
    mail_sendmail_command: value.mail.sendmail_command,
    mail_smtp_host: value.mail.smtp_host,
    mail_smtp_port: value.mail.smtp_port,
    mail_smtp_security: value.mail.smtp_security,
    mail_smtp_username: value.mail.smtp_username,
    session_cookie_secure: value.session.cookie_secure,
    session_idle_ttl_seconds: value.session.idle_ttl_seconds,
    session_ttl_seconds: value.session.session_ttl_seconds,
    audit_retention_days: value.audit.retention_days,
    node_manager_auto_start_local_node: value.node_manager.auto_start_local_node,
    node_manager_local_node_binary: value.node_manager.local_node_binary,
    node_manager_local_node_config: value.node_manager.local_node_config,
    node_manager_restart_on_exit: value.node_manager.restart_on_exit,
    migration_auto_migrate: value.migration.auto_migrate,
    log_level: value.log.level,
    log_format: value.log.format,
  };
}
