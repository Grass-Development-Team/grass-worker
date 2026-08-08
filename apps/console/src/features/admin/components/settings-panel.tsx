import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SettingsCard } from "@/components/settings-card";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { useBranding } from "@/features/branding/branding-context";

import { adminApi, type AdminSettings } from "../admin.api";
import { StorageSettingsPanel } from "./storage-settings-panel";

type UpdateSettingsInput = Parameters<typeof adminApi.updateSettings>[0];

export type SettingsSection =
  | "all"
  | "basic"
  | "email"
  | "governance"
  | "infrastructure"
  | "runtime";

function SettingsSectionView({
  section,
  visible,
  children,
}: {
  section: SettingsSection;
  visible: SettingsSection;
  children: React.ReactNode;
}) {
  return section === "all" || section === visible ? <>{children}</> : null;
}

export function SettingsPanel({ section = "all" }: { section?: SettingsSection }) {
  const settingsQuery = useQuery({
    queryKey: ["admin", "settings"],
    queryFn: adminApi.getSettings,
  });

  if (settingsQuery.isLoading) {
    return <Skeleton className="h-64 w-full" aria-busy="true" />;
  }
  if (settingsQuery.isError || !settingsQuery.data) return null;
  return <SettingsForm initial={settingsQuery.data} section={section} />;
}

function useSettingsMutation() {
  const queryClient = useQueryClient();
  const [saved, setSaved] = useState(false);
  const mutation = useMutation({
    mutationFn: (input: UpdateSettingsInput) => adminApi.updateSettings(input),
    onSuccess: () => {
      setSaved(true);
      queryClient.invalidateQueries({ queryKey: ["admin", "settings"] });
      queryClient.invalidateQueries({ queryKey: ["site-config"] });
    },
  });
  return { mutation, saved, setSaved };
}

function SaveAction({ pending, saved }: { pending: boolean; saved: boolean }) {
  return (
    <>
      {saved && !pending && <span className="text-xs text-muted-foreground">Saved.</span>}
      <Button type="submit" size="sm" disabled={pending}>
        {pending ? "Saving…" : "Save"}
      </Button>
    </>
  );
}

function BooleanSetting({
  id,
  label,
  description,
  checked,
  onCheckedChange,
}: {
  id: string;
  label: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        {description && <FieldDescription>{description}</FieldDescription>}
      </FieldContent>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </Field>
  );
}

function ConfigurationStatus({ label, configured }: { label: string; configured: boolean }) {
  return (
    <Field orientation="horizontal">
      <FieldTitle>{label}</FieldTitle>
      <Badge variant={configured ? "success" : "secondary"}>
        {configured ? "Configured" : "Not configured"}
      </Badge>
    </Field>
  );
}

function SettingsForm({ initial, section }: { initial: AdminSettings; section: SettingsSection }) {
  const { siteName: configuredSiteName } = useBranding();
  const [siteName, setSiteName] = useState(initial.site.name ?? "");
  const [siteLogoUrl, setSiteLogoUrl] = useState(initial.site.logo_url ?? "");
  const [siteUrl, setSiteUrl] = useState(initial.site.url ?? "");
  const [publicBaseUrl, setPublicBaseUrl] = useState(initial.site.public_base_url ?? "");
  const [signupPolicy, setSignupPolicy] = useState(initial.signup.policy);
  const [reviewProduction, setReviewProduction] = useState(initial.review.production);
  const [reviewPreview, setReviewPreview] = useState(initial.review.preview);
  const [domainReview, setDomainReview] = useState(initial.domain_review.default);
  const [serverHost, setServerHost] = useState(initial.server.host);
  const [serverPort, setServerPort] = useState(initial.server.port);
  const [redisBackend, setRedisBackend] = useState(initial.redis.backend);
  const [cookieSecure, setCookieSecure] = useState(initial.session.cookie_secure);
  const [sessionIdleTtl, setSessionIdleTtl] = useState(initial.session.idle_ttl_seconds);
  const [sessionTtl, setSessionTtl] = useState(initial.session.session_ttl_seconds);
  const [auditRetentionDays, setAuditRetentionDays] = useState(initial.audit.retention_days);
  const [mailMode, setMailMode] = useState(initial.mail.mode);
  const [mailFromAddress, setMailFromAddress] = useState(initial.mail.from_address);
  const [mailFromName, setMailFromName] = useState(initial.mail.from_name);
  const [sendmailCommand, setSendmailCommand] = useState(initial.mail.sendmail_command);
  const [smtpHost, setSmtpHost] = useState(initial.mail.smtp_host);
  const [smtpPort, setSmtpPort] = useState(initial.mail.smtp_port);
  const [smtpSecurity, setSmtpSecurity] = useState(initial.mail.smtp_security);
  const [smtpUsername, setSmtpUsername] = useState(initial.mail.smtp_username);
  const [smtpPassword, setSmtpPassword] = useState("");
  const [autoStartLocalNode, setAutoStartLocalNode] = useState(
    initial.node_manager.auto_start_local_node,
  );
  const [localNodeBinary, setLocalNodeBinary] = useState(initial.node_manager.local_node_binary);
  const [localNodeConfig, setLocalNodeConfig] = useState(initial.node_manager.local_node_config);
  const [restartLocalNodeOnExit, setRestartLocalNodeOnExit] = useState(
    initial.node_manager.restart_on_exit,
  );
  const [autoMigrate, setAutoMigrate] = useState(initial.migration.auto_migrate);
  const [logLevel, setLogLevel] = useState(initial.log.level);
  const [logFormat, setLogFormat] = useState(initial.log.format);

  const site = useSettingsMutation();
  const policies = useSettingsMutation();
  const server = useSettingsMutation();
  const sessions = useSettingsMutation();
  const mail = useSettingsMutation();
  const nodeManager = useSettingsMutation();
  const startup = useSettingsMutation();

  return (
    <div className="flex flex-col gap-6">
      <SettingsSectionView section={section} visible="basic">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            site.mutation.mutate({
              site_name: siteName,
              site_logo_url: siteLogoUrl,
              site_url: siteUrl,
              public_base_url: publicBaseUrl,
            });
          }}
          onChange={() => site.setSaved(false)}
        >
          <SettingsCard
            title="Site"
            description={`Identity and URLs of this ${configuredSiteName} installation.`}
            hint="The public base URL is used in generated links."
            action={<SaveAction pending={site.mutation.isPending} saved={site.saved} />}
          >
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="settings-site-name">Site name</FieldLabel>
                <Input
                  id="settings-site-name"
                  value={siteName}
                  onChange={(event) => setSiteName(event.target.value)}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-site-logo-url">Logo URL</FieldLabel>
                <Input
                  id="settings-site-logo-url"
                  value={siteLogoUrl}
                  onChange={(event) => setSiteLogoUrl(event.target.value)}
                  placeholder="https://cdn.example.com/logo.svg"
                />
                <FieldDescription>Use an HTTPS URL or a path served by this site.</FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-site-url">Console URL</FieldLabel>
                <Input
                  id="settings-site-url"
                  type="url"
                  value={siteUrl}
                  onChange={(event) => setSiteUrl(event.target.value)}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-public-base-url">Public base URL</FieldLabel>
                <Input
                  id="settings-public-base-url"
                  type="url"
                  value={publicBaseUrl}
                  onChange={(event) => setPublicBaseUrl(event.target.value)}
                  required
                />
              </Field>
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="email">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            mail.mutation.mutate({
              mail_mode: mailMode,
              mail_from_address: mailFromAddress,
              mail_from_name: mailFromName,
              mail_sendmail_command: sendmailCommand,
              mail_smtp_host: smtpHost,
              mail_smtp_port: smtpPort,
              mail_smtp_security: smtpSecurity,
              mail_smtp_username: smtpUsername,
              ...(smtpPassword ? { mail_smtp_password: smtpPassword } : {}),
            });
          }}
          onChange={() => mail.setSaved(false)}
        >
          <SettingsCard
            title="Email delivery"
            description="Platform mail transport for account and deployment messages."
            hint="SMTP passwords are write-only. Leaving the password blank preserves the configured value."
            action={<SaveAction pending={mail.mutation.isPending} saved={mail.saved} />}
          >
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="settings-mail-mode">Delivery mode</FieldLabel>
                <Select
                  value={mailMode}
                  onValueChange={(value) => {
                    setMailMode(value as typeof mailMode);
                    mail.setSaved(false);
                  }}
                >
                  <SelectTrigger id="settings-mail-mode">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">Disabled</SelectItem>
                    <SelectItem value="local">Local MTA</SelectItem>
                    <SelectItem value="smtp">SMTP</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
              <div className="grid gap-4 sm:grid-cols-2">
                <Field>
                  <FieldLabel htmlFor="settings-mail-from-address">From address</FieldLabel>
                  <Input
                    id="settings-mail-from-address"
                    type="email"
                    value={mailFromAddress}
                    onChange={(event) => setMailFromAddress(event.target.value)}
                    required
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="settings-mail-from-name">From name</FieldLabel>
                  <Input
                    id="settings-mail-from-name"
                    value={mailFromName}
                    onChange={(event) => setMailFromName(event.target.value)}
                    required
                  />
                </Field>
              </div>
              {mailMode === "local" && (
                <Field>
                  <FieldLabel htmlFor="settings-sendmail-command">Sendmail command</FieldLabel>
                  <Input
                    id="settings-sendmail-command"
                    value={sendmailCommand}
                    onChange={(event) => setSendmailCommand(event.target.value)}
                    required
                  />
                </Field>
              )}
              {mailMode === "smtp" && (
                <>
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field>
                      <FieldLabel htmlFor="settings-smtp-host">SMTP host</FieldLabel>
                      <Input
                        id="settings-smtp-host"
                        value={smtpHost}
                        onChange={(event) => setSmtpHost(event.target.value)}
                        required
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="settings-smtp-port">SMTP port</FieldLabel>
                      <Input
                        id="settings-smtp-port"
                        type="number"
                        min={1}
                        max={65_535}
                        value={smtpPort}
                        onChange={(event) => setSmtpPort(Number(event.target.value))}
                        required
                      />
                    </Field>
                  </div>
                  <Field>
                    <FieldLabel htmlFor="settings-smtp-security">SMTP security</FieldLabel>
                    <Select
                      value={smtpSecurity}
                      onValueChange={(value) => {
                        setSmtpSecurity(value as typeof smtpSecurity);
                        mail.setSaved(false);
                      }}
                    >
                      <SelectTrigger id="settings-smtp-security">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="starttls">STARTTLS</SelectItem>
                        <SelectItem value="tls">Implicit TLS</SelectItem>
                        <SelectItem value="none">None</SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>
                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field>
                      <FieldLabel htmlFor="settings-smtp-username">SMTP username</FieldLabel>
                      <Input
                        id="settings-smtp-username"
                        value={smtpUsername}
                        onChange={(event) => setSmtpUsername(event.target.value)}
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="settings-smtp-password">SMTP password</FieldLabel>
                      <Input
                        id="settings-smtp-password"
                        type="password"
                        value={smtpPassword}
                        onChange={(event) => setSmtpPassword(event.target.value)}
                        placeholder={initial.mail.smtp_password_configured ? "Configured" : ""}
                        autoComplete="new-password"
                      />
                    </Field>
                  </div>
                </>
              )}
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="infrastructure">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            server.mutation.mutate({
              server_host: serverHost,
              server_port: serverPort,
              redis_backend: redisBackend,
            });
          }}
          onChange={() => server.setSaved(false)}
        >
          <SettingsCard
            title="Server and cache"
            description="Control API listener and cache backend."
            hint="Saving these settings requires a Control API restart. Environment variables can still override persisted values."
            action={<SaveAction pending={server.mutation.isPending} saved={server.saved} />}
          >
            <FieldGroup>
              <div className="grid gap-4 sm:grid-cols-2">
                <Field>
                  <FieldLabel htmlFor="settings-server-host">Server host</FieldLabel>
                  <Input
                    id="settings-server-host"
                    value={serverHost}
                    onChange={(event) => setServerHost(event.target.value)}
                    required
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="settings-server-port">Server port</FieldLabel>
                  <Input
                    id="settings-server-port"
                    type="number"
                    min={1}
                    max={65_535}
                    value={serverPort}
                    onChange={(event) => setServerPort(Number(event.target.value))}
                    required
                  />
                </Field>
              </div>
              <Field>
                <FieldLabel htmlFor="settings-cache-backend">Cache backend</FieldLabel>
                <Select
                  value={redisBackend}
                  onValueChange={(value) => {
                    setRedisBackend(value as typeof redisBackend);
                    server.setSaved(false);
                  }}
                >
                  <SelectTrigger id="settings-cache-backend">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="moka">Moka — in-process</SelectItem>
                    <SelectItem value="redis">Redis</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="governance">
        <SettingsCard
          title="Sensitive configuration"
          description="Secret values are never returned to the Console. Configure them through the deployment environment or configuration file."
        >
          <FieldGroup className="gap-4">
            <ConfigurationStatus
              label="Database URL"
              configured={initial.database.url_configured}
            />
            <ConfigurationStatus label="Redis URL" configured={initial.redis.url_configured} />
            <ConfigurationStatus
              label="Control API secret"
              configured={initial.secrets.secret_key_configured}
            />
            <ConfigurationStatus
              label="Git credential encryption"
              configured={initial.secrets.git_credentials_configured}
            />
          </FieldGroup>
        </SettingsCard>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="governance">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            sessions.mutation.mutate({
              session_cookie_secure: cookieSecure,
              session_idle_ttl_seconds: sessionIdleTtl,
              session_ttl_seconds: sessionTtl,
              audit_retention_days: auditRetentionDays,
            });
          }}
          onChange={() => sessions.setSaved(false)}
        >
          <SettingsCard
            title="Sessions and audit"
            description="Authentication lifetimes and audit retention."
            hint="These settings apply immediately. Set audit retention to 0 to keep events permanently."
            action={<SaveAction pending={sessions.mutation.isPending} saved={sessions.saved} />}
          >
            <FieldGroup>
              <BooleanSetting
                id="settings-cookie-secure"
                label="Secure session cookies"
                description="Only send authentication cookies over HTTPS."
                checked={cookieSecure}
                onCheckedChange={(checked) => {
                  setCookieSecure(checked);
                  sessions.setSaved(false);
                }}
              />
              <div className="grid gap-4 sm:grid-cols-2">
                <Field>
                  <FieldLabel htmlFor="settings-session-idle-ttl">
                    Session idle TTL (seconds)
                  </FieldLabel>
                  <Input
                    id="settings-session-idle-ttl"
                    type="number"
                    min={1}
                    value={sessionIdleTtl}
                    onChange={(event) => setSessionIdleTtl(Number(event.target.value))}
                    required
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="settings-session-ttl">
                    Absolute session TTL (seconds)
                  </FieldLabel>
                  <Input
                    id="settings-session-ttl"
                    type="number"
                    min={1}
                    value={sessionTtl}
                    onChange={(event) => setSessionTtl(Number(event.target.value))}
                    required
                  />
                </Field>
              </div>
              <Field>
                <FieldLabel htmlFor="settings-audit-retention">Audit retention (days)</FieldLabel>
                <Input
                  id="settings-audit-retention"
                  type="number"
                  min={0}
                  value={auditRetentionDays}
                  onChange={(event) => setAuditRetentionDays(Number(event.target.value))}
                  required
                />
              </Field>
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="runtime">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            nodeManager.mutation.mutate({
              node_manager_auto_start_local_node: autoStartLocalNode,
              node_manager_local_node_binary: localNodeBinary,
              node_manager_local_node_config: localNodeConfig,
              node_manager_restart_on_exit: restartLocalNodeOnExit,
            });
          }}
          onChange={() => nodeManager.setSaved(false)}
        >
          <SettingsCard
            title="Local Node manager"
            description="How this Control API supervises a Node process on the same machine."
            hint="Saving these settings requires a Control API restart."
            action={
              <SaveAction pending={nodeManager.mutation.isPending} saved={nodeManager.saved} />
            }
          >
            <FieldGroup>
              <BooleanSetting
                id="settings-node-auto-start"
                label="Auto-start local Node"
                checked={autoStartLocalNode}
                onCheckedChange={(checked) => {
                  setAutoStartLocalNode(checked);
                  nodeManager.setSaved(false);
                }}
              />
              <Field>
                <FieldLabel htmlFor="settings-node-binary">Local Node binary</FieldLabel>
                <Input
                  id="settings-node-binary"
                  value={localNodeBinary}
                  onChange={(event) => setLocalNodeBinary(event.target.value)}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-node-config">Local Node config</FieldLabel>
                <Input
                  id="settings-node-config"
                  value={localNodeConfig}
                  onChange={(event) => setLocalNodeConfig(event.target.value)}
                  required
                />
              </Field>
              <BooleanSetting
                id="settings-node-restart-on-exit"
                label="Restart local Node on exit"
                checked={restartLocalNodeOnExit}
                onCheckedChange={(checked) => {
                  setRestartLocalNodeOnExit(checked);
                  nodeManager.setSaved(false);
                }}
              />
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="runtime">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            startup.mutation.mutate({
              migration_auto_migrate: autoMigrate,
              log_level: logLevel,
              log_format: logFormat,
            });
          }}
          onChange={() => startup.setSaved(false)}
        >
          <SettingsCard
            title="Startup and logging"
            description="Database migration and process log output."
            hint="Saving these settings requires a Control API restart."
            action={<SaveAction pending={startup.mutation.isPending} saved={startup.saved} />}
          >
            <FieldGroup>
              <BooleanSetting
                id="settings-auto-migrate"
                label="Run database migrations on startup"
                checked={autoMigrate}
                onCheckedChange={(checked) => {
                  setAutoMigrate(checked);
                  startup.setSaved(false);
                }}
              />
              <Field>
                <FieldLabel htmlFor="settings-log-level">Log filter</FieldLabel>
                <Input
                  id="settings-log-level"
                  value={logLevel}
                  onChange={(event) => setLogLevel(event.target.value)}
                  required
                />
                <FieldDescription>
                  A tracing filter such as info or warn,grass_control_api=debug.
                </FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="settings-log-format">Log format</FieldLabel>
                <Select
                  value={logFormat}
                  onValueChange={(value) => {
                    setLogFormat(value as typeof logFormat);
                    startup.setSaved(false);
                  }}
                >
                  <SelectTrigger id="settings-log-format">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="pretty">Pretty</SelectItem>
                    <SelectItem value="json">JSON</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="infrastructure">
        <StorageSettingsPanel />
      </SettingsSectionView>

      <SettingsSectionView section={section} visible="governance">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            policies.mutation.mutate({
              signup_policy: signupPolicy,
              review_production: reviewProduction,
              review_preview: reviewPreview,
              domain_review_default: domainReview,
            });
          }}
        >
          <SettingsCard
            title="Policies"
            description="Signup, release review, and custom domain review defaults."
            hint="Review policy changes apply to new deployments and custom domains."
            action={<SaveAction pending={policies.mutation.isPending} saved={policies.saved} />}
          >
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="settings-signup-policy">Signup policy</FieldLabel>
                <Select
                  value={signupPolicy}
                  onValueChange={(value) => {
                    setSignupPolicy(value as typeof signupPolicy);
                    policies.setSaved(false);
                  }}
                >
                  <SelectTrigger id="settings-signup-policy">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="open">Open — anyone can register</SelectItem>
                    <SelectItem value="invite_only">Invite only</SelectItem>
                    <SelectItem value="closed">Closed</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
              <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                <Field>
                  <FieldLabel htmlFor="settings-review-production">Production review</FieldLabel>
                  <Select
                    value={reviewProduction}
                    onValueChange={(value) => {
                      setReviewProduction(value as typeof reviewProduction);
                      policies.setSaved(false);
                    }}
                  >
                    <SelectTrigger id="settings-review-production">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="manual">Manual — requires approval</SelectItem>
                      <SelectItem value="auto">Auto — activates when ready</SelectItem>
                    </SelectContent>
                  </Select>
                </Field>
                <Field>
                  <FieldLabel htmlFor="settings-review-preview">Preview review</FieldLabel>
                  <Select
                    value={reviewPreview}
                    onValueChange={(value) => {
                      setReviewPreview(value as typeof reviewPreview);
                      policies.setSaved(false);
                    }}
                  >
                    <SelectTrigger id="settings-review-preview">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">Auto — activates when ready</SelectItem>
                      <SelectItem value="manual">Manual — requires approval</SelectItem>
                    </SelectContent>
                  </Select>
                </Field>
                <Field>
                  <FieldLabel htmlFor="settings-review-domain">Custom domain review</FieldLabel>
                  <Select
                    value={domainReview}
                    onValueChange={(value) => {
                      setDomainReview(value as typeof domainReview);
                      policies.setSaved(false);
                    }}
                  >
                    <SelectTrigger id="settings-review-domain">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">Auto — approve new custom domains</SelectItem>
                      <SelectItem value="manual">Manual — requires approval</SelectItem>
                    </SelectContent>
                  </Select>
                </Field>
              </div>
            </FieldGroup>
          </SettingsCard>
        </form>
      </SettingsSectionView>
    </div>
  );
}
