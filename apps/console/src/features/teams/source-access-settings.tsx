import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { SettingsCard } from "@/components/settings-card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

import { canManageMembers } from "./team-permissions";
import { useTeam } from "./team-context";
import { teamsApi, type SourceCredential } from "./teams.api";

export function SourceAccessSettings() {
  const { activeTeam, activeRole } = useTeam();
  const queryClient = useQueryClient();
  const canManage = Boolean(activeRole && canManageMembers(activeRole));
  const teamId = activeTeam?.id;
  const credentialsKey = ["source-credentials", teamId];
  const hostKeysKey = ["ssh-host-keys", teamId];

  const credentials = useQuery({
    queryKey: credentialsKey,
    queryFn: () => teamsApi.listSourceCredentials(teamId!),
    enabled: Boolean(teamId && canManage),
  });
  const hostKeys = useQuery({
    queryKey: hostKeysKey,
    queryFn: () => teamsApi.listSshHostKeys(teamId!),
    enabled: Boolean(teamId && canManage),
  });

  const [editing, setEditing] = useState<SourceCredential | null>(null);
  const [kind, setKind] = useState<"https" | "ssh">("https");
  const [name, setName] = useState("");
  const [repositoryUrl, setRepositoryUrl] = useState("");
  const [username, setUsername] = useState("");
  const [secret, setSecret] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const [passphrase, setPassphrase] = useState("");

  const clearSecretForm = () => {
    setEditing(null);
    setName("");
    setRepositoryUrl("");
    setUsername("");
    setSecret("");
    setPrivateKey("");
    setPassphrase("");
  };

  const saveCredential = useMutation({
    mutationFn: () => {
      const input = {
        username: username.trim(),
        ...(kind === "https"
          ? { secret }
          : { private_key: privateKey, passphrase: passphrase || undefined }),
      };
      return editing
        ? teamsApi.rotateSourceCredential(teamId!, editing.id, input)
        : teamsApi.createSourceCredential(teamId!, {
            ...input,
            name: name.trim(),
            repository_url: repositoryUrl.trim(),
          });
    },
    onSuccess: async () => {
      clearSecretForm();
      await queryClient.invalidateQueries({ queryKey: credentialsKey });
    },
  });

  const revokeCredential = useMutation({
    mutationFn: (credentialId: string) => teamsApi.revokeSourceCredential(teamId!, credentialId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: credentialsKey }),
  });

  const changeHostKey = useMutation({
    mutationFn: ({ keyId, action }: { keyId: string; action: "approve" | "reject" }) =>
      action === "approve"
        ? teamsApi.approveSshHostKey(teamId!, keyId)
        : teamsApi.rejectSshHostKey(teamId!, keyId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: hostKeysKey }),
  });

  if (!canManage) {
    return (
      <SettingsCard
        title="Git source access"
        description="Private repository credentials and SSH host keys."
        hint="Only team owners and admins can manage source access."
      >
        <p className="text-sm text-muted-foreground">
          Ask a team owner or admin to configure private repository access.
        </p>
      </SettingsCard>
    );
  }

  return (
    <>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          saveCredential.mutate();
        }}
      >
        <SettingsCard
          title="Git credentials"
          description="Team credentials are encrypted and can be bound to compatible projects."
          hint="Secrets are write-only. Rotation does not change credentials already fixed to queued deployments."
          action={
            <div className="flex gap-2">
              {editing && (
                <Button type="button" size="sm" variant="outline" onClick={clearSecretForm}>
                  Cancel
                </Button>
              )}
              <Button
                type="submit"
                size="sm"
                disabled={
                  saveCredential.isPending ||
                  !username.trim() ||
                  (kind === "https" ? !secret : !privateKey.trim()) ||
                  (!editing && (!name.trim() || !repositoryUrl.trim()))
                }
              >
                {saveCredential.isPending ? "Saving…" : editing ? "Rotate" : "Add credential"}
              </Button>
            </div>
          }
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel>Type</FieldLabel>
              <Select
                value={kind}
                disabled={Boolean(editing)}
                onValueChange={(value) => setKind(value as "https" | "ssh")}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="https">HTTPS token / password</SelectItem>
                  <SelectItem value="ssh">SSH private key</SelectItem>
                </SelectContent>
              </Select>
            </Field>
            {!editing && (
              <Field>
                <FieldLabel htmlFor="credential-name">Name</FieldLabel>
                <Input
                  id="credential-name"
                  value={name}
                  placeholder="GitHub deploy token"
                  onChange={(event) => setName(event.target.value)}
                />
              </Field>
            )}
            {!editing && (
              <Field className="sm:col-span-2">
                <FieldLabel htmlFor="credential-endpoint">Repository URL for scope</FieldLabel>
                <Input
                  id="credential-endpoint"
                  value={repositoryUrl}
                  placeholder={
                    kind === "https"
                      ? "https://github.com/acme/site.git"
                      : "ssh://git@git.example.com:2222/acme/site.git"
                  }
                  onChange={(event) => setRepositoryUrl(event.target.value)}
                />
                <FieldDescription>
                  Only scheme, host, and effective port define the scope.
                </FieldDescription>
              </Field>
            )}
            <Field>
              <FieldLabel htmlFor="credential-username">Username</FieldLabel>
              <Input
                id="credential-username"
                value={username}
                autoComplete="off"
                onChange={(event) => setUsername(event.target.value)}
              />
            </Field>
            {kind === "https" ? (
              <Field>
                <FieldLabel htmlFor="credential-secret">Token or password</FieldLabel>
                <Input
                  id="credential-secret"
                  type="password"
                  value={secret}
                  autoComplete="new-password"
                  onChange={(event) => setSecret(event.target.value)}
                />
              </Field>
            ) : (
              <>
                <Field className="sm:col-span-2">
                  <FieldLabel htmlFor="credential-private-key">Private key</FieldLabel>
                  <Textarea
                    id="credential-private-key"
                    value={privateKey}
                    autoComplete="off"
                    onChange={(event) => setPrivateKey(event.target.value)}
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="credential-passphrase">Passphrase (optional)</FieldLabel>
                  <Input
                    id="credential-passphrase"
                    type="password"
                    value={passphrase}
                    autoComplete="new-password"
                    onChange={(event) => setPassphrase(event.target.value)}
                  />
                </Field>
              </>
            )}
          </div>

          <div className="mt-6 space-y-2 border-t pt-4">
            {credentials.data?.credentials.map((credential) => (
              <div
                key={credential.id}
                className="flex flex-col gap-3 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="truncate text-sm font-medium">{credential.name}</p>
                    <Badge variant={credential.revoked_at ? "destructive" : "secondary"}>
                      {credential.revoked_at ? "Revoked" : credential.kind.toUpperCase()}
                    </Badge>
                  </div>
                  <p className="truncate text-xs text-muted-foreground">
                    {credential.username}@{credential.host}:{credential.port}
                  </p>
                </div>
                {!credential.revoked_at && (
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setEditing(credential);
                        setKind(credential.kind);
                        setUsername(credential.username ?? "");
                        setSecret("");
                        setPrivateKey("");
                        setPassphrase("");
                      }}
                    >
                      Rotate
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="destructive"
                      disabled={revokeCredential.isPending}
                      onClick={() => {
                        if (
                          globalThis.confirm("Revoke this credential and invalidate its versions?")
                        ) {
                          revokeCredential.mutate(credential.id);
                        }
                      }}
                    >
                      Revoke
                    </Button>
                  </div>
                )}
              </div>
            ))}
            {credentials.data?.credentials.length === 0 && (
              <p className="text-sm text-muted-foreground">No source credentials configured.</p>
            )}
          </div>
        </SettingsCard>
      </form>

      <SettingsCard
        title="SSH host keys"
        description="First-use fingerprints must be approved before a Node can clone over SSH."
        hint="A changed key creates a new pending fingerprint and blocks checkout until reapproved."
      >
        <div className="space-y-2">
          {hostKeys.data?.host_keys.map((key) => (
            <div
              key={key.id}
              className="flex flex-col gap-3 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0">
                <p className="text-sm font-medium">
                  {key.host}:{key.port} · {key.key_type}
                </p>
                <p className="break-all font-mono text-xs text-muted-foreground">
                  {key.fingerprint_sha256}
                </p>
              </div>
              <div className="flex items-center gap-2">
                <Badge variant={key.status === "pending" ? "outline" : "secondary"}>
                  {key.status}
                </Badge>
                {key.status !== "approved" && (
                  <>
                    {key.status !== "rejected" && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => changeHostKey.mutate({ keyId: key.id, action: "reject" })}
                      >
                        Reject
                      </Button>
                    )}
                    <Button
                      size="sm"
                      onClick={() => changeHostKey.mutate({ keyId: key.id, action: "approve" })}
                    >
                      Approve
                    </Button>
                  </>
                )}
              </div>
            </div>
          ))}
          {hostKeys.data?.host_keys.length === 0 && (
            <p className="text-sm text-muted-foreground">
              No SSH fingerprints have been observed yet.
            </p>
          )}
        </div>
      </SettingsCard>
    </>
  );
}
