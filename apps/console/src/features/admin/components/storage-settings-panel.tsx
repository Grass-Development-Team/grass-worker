import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2Icon,
  CloudIcon,
  DatabaseBackupIcon,
  HardDriveIcon,
  PlugZapIcon,
} from "lucide-react";
import { useState } from "react";

import { SettingsCard } from "@/components/settings-card";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";

import {
  adminApi,
  type AdminStorageBackend,
  type AdminStorageConfiguration,
  type AdminStorageInput,
  type AdminStorageMigration,
  type AdminStorageState,
} from "../admin.api";

const ACTIVE_MIGRATION_STATUSES = new Set<AdminStorageMigration["status"]>(["pending", "running"]);

const PROVIDER_LABELS: Record<AdminStorageBackend, string> = {
  local: "Local filesystem",
  s3: "S3-compatible",
  minio: "MinIO",
  r2: "Cloudflare R2",
};

function defaultRegion(backend: AdminStorageBackend) {
  return backend === "r2" ? "auto" : "us-east-1";
}

function migrationProgress(migration: AdminStorageMigration) {
  if (migration.total_objects === 0) {
    return migration.status === "succeeded" ? 100 : 0;
  }
  if (migration.total_objects === null) return 0;
  return Math.min(100, Math.round((migration.copied_objects / migration.total_objects) * 100));
}

function CurrentStorage({ storage }: { storage: AdminStorageConfiguration }) {
  const remote = storage.backend !== "local";
  return (
    <div className="grid gap-3 border-b pb-5 sm:grid-cols-2">
      <div className="flex items-start gap-3">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted/40">
          {remote ? <CloudIcon className="size-4" /> : <HardDriveIcon className="size-4" />}
        </div>
        <div className="min-w-0">
          <p className="text-sm font-medium">{PROVIDER_LABELS[storage.backend]}</p>
          <p className="break-all text-xs text-muted-foreground">
            {remote ? storage.bucket : storage.local_root}
          </p>
        </div>
      </div>
      <div className="flex flex-wrap items-start gap-2 sm:justify-end">
        {remote && <Badge variant="secondary">{storage.region}</Badge>}
        {remote && (
          <Badge variant="outline">
            {storage.force_path_style ? "Path-style requests" : "Virtual-hosted requests"}
          </Badge>
        )}
        {remote && (
          <Badge variant={storage.allow_http ? "warning" : "success"}>
            {storage.allow_http ? "HTTP allowed" : "HTTPS only"}
          </Badge>
        )}
        {storage.credentials_configured && <Badge variant="success">Credentials configured</Badge>}
      </div>
      {remote && (
        <dl className="grid gap-2 text-xs text-muted-foreground sm:col-span-2 sm:grid-cols-3">
          <div className="min-w-0">
            <dt className="font-medium text-foreground">Prefix</dt>
            <dd className="break-all">{storage.prefix || "/"}</dd>
          </div>
          <div className="min-w-0">
            <dt className="font-medium text-foreground">Endpoint</dt>
            <dd className="break-all">{storage.endpoint || "Provider default"}</dd>
          </div>
          <div className="min-w-0">
            <dt className="font-medium text-foreground">Local node root</dt>
            <dd className="break-all">{storage.local_root}</dd>
          </div>
        </dl>
      )}
    </div>
  );
}

function MigrationStatus({ migration }: { migration: AdminStorageMigration }) {
  const progress = migrationProgress(migration);
  const total = migration.total_objects;
  return (
    <div className="grid gap-3 border-b pb-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-sm font-medium">
            {migration.status === "succeeded"
              ? "Migration completed"
              : migration.status === "failed"
                ? "Migration failed"
                : "Object writes are paused"}
          </p>
          <p className="text-xs text-muted-foreground">
            {PROVIDER_LABELS[migration.source.backend]} to{" "}
            {PROVIDER_LABELS[migration.target.backend]}
          </p>
        </div>
        <Badge
          variant={
            migration.status === "succeeded"
              ? "success"
              : migration.status === "failed"
                ? "destructive"
                : "warning"
          }
        >
          {migration.status}
        </Badge>
      </div>
      <Progress value={progress} aria-valuenow={progress} aria-label="Storage migration progress" />
      <div className="flex flex-wrap justify-between gap-2 text-xs text-muted-foreground">
        <span>
          {migration.copied_objects} of {total ?? "unknown"} objects
        </span>
        <span>{progress}%</span>
      </div>
      {migration.last_error && (
        <p role="alert" className="text-sm text-destructive">
          {migration.last_error}
        </p>
      )}
    </div>
  );
}

export function StorageSettingsPanel() {
  const queryClient = useQueryClient();
  const [backend, setBackend] = useState<AdminStorageBackend>("local");
  const [localRoot, setLocalRoot] = useState<string | null>(null);
  const [endpoint, setEndpoint] = useState("");
  const [region, setRegion] = useState(defaultRegion("local"));
  const [bucket, setBucket] = useState("");
  const [prefix, setPrefix] = useState("");
  const [forcePathStyle, setForcePathStyle] = useState(false);
  const [allowHttp, setAllowHttp] = useState(false);
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [sessionToken, setSessionToken] = useState("");
  const [connectionVerified, setConnectionVerified] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const storageQuery = useQuery({
    queryKey: ["admin", "storage"],
    queryFn: adminApi.getStorage,
    refetchInterval: (query) =>
      query.state.data?.migration &&
      ACTIVE_MIGRATION_STATUSES.has(query.state.data.migration.status)
        ? 2_000
        : false,
  });

  const currentRoot = localRoot ?? storageQuery.data?.storage.local_root ?? "/data";
  const remote = backend !== "local";
  const maintenance = storageQuery.data?.maintenance ?? false;

  const input = (): AdminStorageInput => {
    if (backend === "local") {
      return { backend, local_root: currentRoot.trim() };
    }
    return {
      backend,
      local_root: currentRoot.trim(),
      endpoint: endpoint.trim(),
      region: region.trim(),
      bucket: bucket.trim(),
      prefix: prefix.trim().replace(/^\/+|\/+$/g, ""),
      force_path_style: forcePathStyle,
      allow_http: allowHttp,
      ...(accessKeyId.trim() && { access_key_id: accessKeyId.trim() }),
      ...(secretAccessKey.trim() && { secret_access_key: secretAccessKey.trim() }),
      ...(sessionToken.trim() && { session_token: sessionToken.trim() }),
    };
  };

  const testMutation = useMutation({
    mutationFn: () => adminApi.testStorage(input()),
    onSuccess: () => setConnectionVerified(true),
  });
  const migrationMutation = useMutation({
    mutationFn: () => adminApi.createStorageMigration(input()),
    onSuccess: ({ migration }) => {
      setConnectionVerified(false);
      setConfirmOpen(false);
      queryClient.setQueryData<AdminStorageState>(["admin", "storage"], (current) =>
        current ? { ...current, maintenance: true, migration } : current,
      );
      queryClient.invalidateQueries({ queryKey: ["admin", "storage"] });
    },
  });

  const change = (apply: () => void) => {
    apply();
    setConnectionVerified(false);
  };

  const changeBackend = (value: AdminStorageBackend) => {
    change(() => {
      setBackend(value);
      setRegion(defaultRegion(value));
      setForcePathStyle(value === "minio");
      setAllowHttp(value === "minio");
    });
  };

  if (storageQuery.isLoading) {
    return <Skeleton className="h-80 w-full" aria-busy="true" />;
  }
  if (storageQuery.isError || !storageQuery.data) return null;

  return (
    <>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          testMutation.mutate();
        }}
      >
        <SettingsCard
          title="Object storage"
          description="Move managed artifacts, logs, screenshots, and avatars between storage backends."
          hint="Reads continue from the active backend while writes and deletions are paused during migration."
        >
          <div className="grid gap-5">
            <CurrentStorage storage={storageQuery.data.storage} />
            {storageQuery.data.migration && (
              <MigrationStatus migration={storageQuery.data.migration} />
            )}
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="storage-target-backend">Target backend</FieldLabel>
                <Select
                  value={backend}
                  onValueChange={(value) => changeBackend(value as AdminStorageBackend)}
                  disabled={maintenance}
                >
                  <SelectTrigger id="storage-target-backend" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="local">Local storage</SelectItem>
                      <SelectItem value="s3">S3-compatible</SelectItem>
                      <SelectItem value="minio">MinIO</SelectItem>
                      <SelectItem value="r2">Cloudflare R2</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>

              <Field>
                <FieldLabel htmlFor="storage-target-local-root">
                  {remote ? "Target local Node root" : "Target local storage root"}
                </FieldLabel>
                <Input
                  id="storage-target-local-root"
                  value={currentRoot}
                  onChange={(event) => change(() => setLocalRoot(event.target.value))}
                  disabled={maintenance}
                  required
                />
                {remote && (
                  <FieldDescription>
                    Node work directories remain local and are not copied into object storage.
                  </FieldDescription>
                )}
              </Field>

              {remote && (
                <>
                  <Field>
                    <FieldLabel htmlFor="storage-target-endpoint">Target endpoint</FieldLabel>
                    <Input
                      id="storage-target-endpoint"
                      type="url"
                      value={endpoint}
                      onChange={(event) => change(() => setEndpoint(event.target.value))}
                      placeholder={
                        backend === "r2"
                          ? "https://account.r2.cloudflarestorage.com"
                          : "https://s3.example.com"
                      }
                      disabled={maintenance}
                      required={backend === "minio" || backend === "r2"}
                    />
                  </Field>

                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field>
                      <FieldLabel htmlFor="storage-target-region">Target region</FieldLabel>
                      <Input
                        id="storage-target-region"
                        value={region}
                        onChange={(event) => change(() => setRegion(event.target.value))}
                        disabled={maintenance}
                        readOnly={backend === "r2"}
                        required
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="storage-target-bucket">Target bucket</FieldLabel>
                      <Input
                        id="storage-target-bucket"
                        value={bucket}
                        onChange={(event) => change(() => setBucket(event.target.value))}
                        disabled={maintenance}
                        required
                      />
                    </Field>
                  </div>

                  <Field>
                    <FieldLabel htmlFor="storage-target-prefix">Target prefix</FieldLabel>
                    <Input
                      id="storage-target-prefix"
                      value={prefix}
                      onChange={(event) => change(() => setPrefix(event.target.value))}
                      disabled={maintenance}
                      placeholder="grass-worker"
                    />
                  </Field>

                  <div className="grid gap-4 sm:grid-cols-2">
                    <Field>
                      <FieldLabel htmlFor="storage-target-access-key">
                        Target access key ID
                      </FieldLabel>
                      <Input
                        id="storage-target-access-key"
                        value={accessKeyId}
                        onChange={(event) => change(() => setAccessKeyId(event.target.value))}
                        disabled={maintenance}
                        autoComplete="off"
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="storage-target-secret-key">
                        Target secret access key
                      </FieldLabel>
                      <Input
                        id="storage-target-secret-key"
                        type="password"
                        value={secretAccessKey}
                        onChange={(event) => change(() => setSecretAccessKey(event.target.value))}
                        disabled={maintenance}
                        autoComplete="new-password"
                      />
                    </Field>
                  </div>

                  <Field>
                    <FieldLabel htmlFor="storage-target-session-token">
                      Target session token
                    </FieldLabel>
                    <Input
                      id="storage-target-session-token"
                      type="password"
                      value={sessionToken}
                      onChange={(event) => change(() => setSessionToken(event.target.value))}
                      disabled={maintenance}
                      autoComplete="new-password"
                    />
                  </Field>

                  <Field orientation="horizontal">
                    <FieldContent>
                      <FieldLabel htmlFor="storage-target-path-style">
                        Force path-style requests
                      </FieldLabel>
                    </FieldContent>
                    <Switch
                      id="storage-target-path-style"
                      checked={forcePathStyle}
                      onCheckedChange={(checked) => change(() => setForcePathStyle(checked))}
                      disabled={maintenance}
                    />
                  </Field>
                  <Field orientation="horizontal">
                    <FieldContent>
                      <FieldLabel htmlFor="storage-target-allow-http">
                        Allow HTTP endpoint
                      </FieldLabel>
                    </FieldContent>
                    <Switch
                      id="storage-target-allow-http"
                      checked={allowHttp}
                      onCheckedChange={(checked) => change(() => setAllowHttp(checked))}
                      disabled={maintenance}
                    />
                  </Field>
                </>
              )}
            </FieldGroup>
            <div className="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center sm:justify-end">
              {connectionVerified && (
                <span className="inline-flex items-center gap-1 text-xs text-emerald-700 dark:text-emerald-400">
                  <CheckCircle2Icon className="size-3.5" /> Connection verified
                </span>
              )}
              <Button
                type="submit"
                size="sm"
                variant="outline"
                disabled={maintenance || testMutation.isPending || migrationMutation.isPending}
              >
                {testMutation.isPending ? <Spinner data-icon="inline-start" /> : <PlugZapIcon />}
                Test connection
              </Button>
              <Button
                type="button"
                size="sm"
                disabled={!connectionVerified || maintenance || migrationMutation.isPending}
                onClick={() => setConfirmOpen(true)}
              >
                <DatabaseBackupIcon /> Start migration
              </Button>
            </div>
          </div>
        </SettingsCard>
      </form>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Start object storage migration?</AlertDialogTitle>
            <AlertDialogDescription>
              Object writes and deletions will be paused until every managed object is copied and
              verified. Reads continue from the active backend until the final switch.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={migrationMutation.isPending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={migrationMutation.isPending}
              onClick={() => migrationMutation.mutate()}
            >
              {migrationMutation.isPending && <Spinner data-icon="inline-start" />}
              Start migration
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
