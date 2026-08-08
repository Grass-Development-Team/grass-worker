import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { ArrowRight, Package } from "lucide-react";

import {
  setupApi,
  type StorageBackend,
  type StorageConfigurationInput,
} from "@/features/setup/setup.api";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";

function defaultRegion(backend: StorageBackend) {
  return backend === "r2" ? "auto" : "us-east-1";
}

export function StorageStep({ onSuccess }: { onSuccess: () => void }) {
  const [backend, setBackend] = useState<StorageBackend>("local");
  const [localRoot, setLocalRoot] = useState("/data");
  const [endpoint, setEndpoint] = useState("");
  const [region, setRegion] = useState(defaultRegion("local"));
  const [bucket, setBucket] = useState("");
  const [prefix, setPrefix] = useState("");
  const [forcePathStyle, setForcePathStyle] = useState(false);
  const [allowHttp, setAllowHttp] = useState(false);
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [sessionToken, setSessionToken] = useState("");

  const input = (): StorageConfigurationInput => {
    if (backend === "local") {
      return { backend, local_root: localRoot.trim() };
    }
    return {
      backend,
      local_root: localRoot.trim(),
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
  const mutation = useMutation({
    mutationFn: () => setupApi.configureStorage(input()),
    onSuccess,
  });
  const skipMutation = useMutation({
    mutationFn: () => setupApi.configureStorage({ backend: "local", local_root: "/data" }),
    onSuccess,
  });
  const isPending = mutation.isPending || skipMutation.isPending;
  const isRemote = backend !== "local";

  const changeBackend = (value: StorageBackend) => {
    setBackend(value);
    setRegion(defaultRegion(value));
    setForcePathStyle(value === "minio");
    setAllowHttp(value === "minio");
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Package /> Configure Storage
        </CardTitle>
        <CardDescription>
          Choose where artifacts, build logs, screenshots, and avatars are stored.
        </CardDescription>
      </CardHeader>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          mutation.mutate();
        }}
      >
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="storage-backend">Storage backend</FieldLabel>
              <Select
                value={backend}
                onValueChange={(value) => changeBackend(value as StorageBackend)}
              >
                <SelectTrigger id="storage-backend" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="local">Local filesystem</SelectItem>
                    <SelectItem value="s3">S3-compatible</SelectItem>
                    <SelectItem value="minio">MinIO</SelectItem>
                    <SelectItem value="r2">Cloudflare R2</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </Field>

            <Field>
              <FieldLabel htmlFor="storage-local-root">
                {isRemote ? "Local Node root" : "Local storage root"}
              </FieldLabel>
              <Input
                id="storage-local-root"
                value={localRoot}
                onChange={(event) => setLocalRoot(event.target.value)}
                placeholder="/data"
                required
              />
              {isRemote && (
                <FieldDescription>
                  Node work directories remain local and are not part of object migration.
                </FieldDescription>
              )}
            </Field>

            {isRemote && (
              <>
                <Field>
                  <FieldLabel htmlFor="storage-endpoint">Endpoint</FieldLabel>
                  <Input
                    id="storage-endpoint"
                    type="url"
                    value={endpoint}
                    onChange={(event) => setEndpoint(event.target.value)}
                    placeholder={
                      backend === "r2"
                        ? "https://account.r2.cloudflarestorage.com"
                        : "https://s3.example.com"
                    }
                    required={backend === "minio" || backend === "r2"}
                  />
                </Field>

                <div className="grid gap-4 sm:grid-cols-2">
                  <Field>
                    <FieldLabel htmlFor="storage-region">Region</FieldLabel>
                    <Input
                      id="storage-region"
                      value={region}
                      onChange={(event) => setRegion(event.target.value)}
                      required
                      readOnly={backend === "r2"}
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="storage-bucket">Bucket</FieldLabel>
                    <Input
                      id="storage-bucket"
                      value={bucket}
                      onChange={(event) => setBucket(event.target.value)}
                      required
                    />
                  </Field>
                </div>

                <Field>
                  <FieldLabel htmlFor="storage-prefix">Prefix</FieldLabel>
                  <Input
                    id="storage-prefix"
                    value={prefix}
                    onChange={(event) => setPrefix(event.target.value)}
                    placeholder="grass-worker"
                  />
                </Field>

                <div className="grid gap-4 sm:grid-cols-2">
                  <Field>
                    <FieldLabel htmlFor="storage-access-key">Access key ID</FieldLabel>
                    <Input
                      id="storage-access-key"
                      value={accessKeyId}
                      onChange={(event) => setAccessKeyId(event.target.value)}
                      autoComplete="off"
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="storage-secret-key">Secret access key</FieldLabel>
                    <Input
                      id="storage-secret-key"
                      type="password"
                      value={secretAccessKey}
                      onChange={(event) => setSecretAccessKey(event.target.value)}
                      autoComplete="new-password"
                    />
                  </Field>
                </div>

                <Field>
                  <FieldLabel htmlFor="storage-session-token">Session token</FieldLabel>
                  <Input
                    id="storage-session-token"
                    type="password"
                    value={sessionToken}
                    onChange={(event) => setSessionToken(event.target.value)}
                    autoComplete="new-password"
                  />
                </Field>

                <Field orientation="horizontal">
                  <FieldContent>
                    <FieldLabel htmlFor="storage-path-style">Force path-style requests</FieldLabel>
                  </FieldContent>
                  <Switch
                    id="storage-path-style"
                    checked={forcePathStyle}
                    onCheckedChange={setForcePathStyle}
                  />
                </Field>
                <Field orientation="horizontal">
                  <FieldContent>
                    <FieldLabel htmlFor="storage-allow-http">Allow HTTP endpoint</FieldLabel>
                  </FieldContent>
                  <Switch
                    id="storage-allow-http"
                    checked={allowHttp}
                    onCheckedChange={setAllowHttp}
                  />
                </Field>
              </>
            )}
          </FieldGroup>
        </CardContent>
        <CardFooter className="flex flex-col gap-2">
          <Button type="submit" className="w-full" disabled={isPending}>
            {mutation.isPending ? (
              <>
                <Spinner data-icon="inline-start" />
                Testing and saving...
              </>
            ) : (
              <>
                Test and save storage
                <ArrowRight data-icon="inline-end" />
              </>
            )}
          </Button>
          <Button
            type="button"
            variant="ghost"
            className="w-full"
            disabled={isPending}
            onClick={() => skipMutation.mutate()}
          >
            {skipMutation.isPending ? "Skipping..." : "Skip for now (use /data)"}
          </Button>
        </CardFooter>
      </form>
    </Card>
  );
}
