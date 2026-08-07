import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckIcon, CopyIcon, MoreHorizontalIcon, PlusIcon } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  adminApi,
  type AdminCode,
  type AdminCodeStatus,
  type AdminGeneratedCode,
} from "../admin.api";
import { showErrorToast } from "@/lib/toast";

const statusLabels: Record<AdminCodeStatus, string> = {
  available: "Available",
  used: "Used",
  expired: "Expired",
  revoked: "Revoked",
};

function StatusBadge({ status }: { status: AdminCodeStatus }) {
  const variant =
    status === "available"
      ? "success"
      : status === "expired"
        ? "warning"
        : status === "revoked"
          ? "destructive"
          : "secondary";
  return <Badge variant={variant}>{statusLabels[status]}</Badge>;
}

function formatDate(value: string | null) {
  return value ? new Date(value).toLocaleString() : "Never";
}

export function CodePanel() {
  const queryClient = useQueryClient();
  const [scope, setScope] = useState<string>("all");
  const [status, setStatus] = useState<string>("all");
  const [page, setPage] = useState(1);
  const [generateOpen, setGenerateOpen] = useState(false);
  const [generated, setGenerated] = useState<AdminGeneratedCode[] | null>(null);
  const codes = useQuery({
    queryKey: ["admin", "codes", scope, status, page],
    queryFn: () =>
      adminApi.listCodes({
        ...(scope !== "all" ? { scope } : {}),
        ...(status !== "all" ? { status: status as AdminCodeStatus } : {}),
        page,
      }),
  });
  const revoke = useMutation({
    mutationFn: (code: AdminCode) => adminApi.revokeCode(code.id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["admin", "codes"] }),
  });

  const closeGeneration = () => {
    setGenerateOpen(false);
    setGenerated(null);
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-col gap-2 sm:flex-row">
          <Select
            value={scope}
            onValueChange={(value) => {
              setScope(value);
              setPage(1);
            }}
          >
            <SelectTrigger className="w-full sm:w-44" aria-label="Filter by scope">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All scopes</SelectItem>
              {(codes.data?.scopes ?? ["registration"]).map((item) => (
                <SelectItem key={item} value={item}>
                  {item}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select
            value={status}
            onValueChange={(value) => {
              setStatus(value);
              setPage(1);
            }}
          >
            <SelectTrigger className="w-full sm:w-44" aria-label="Filter by status">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All statuses</SelectItem>
              {Object.entries(statusLabels).map(([value, label]) => (
                <SelectItem key={value} value={value}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button onClick={() => setGenerateOpen(true)}>
          <PlusIcon data-icon="inline-start" />
          Generate codes
        </Button>
      </div>

      {codes.isLoading ? (
        <Skeleton className="h-56 w-full" aria-busy="true" />
      ) : codes.data?.codes.length === 0 ? (
        <div className="border-y py-12 text-center text-sm text-muted-foreground">
          No codes found.
        </div>
      ) : (
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Code</TableHead>
                <TableHead>Scope</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Expires</TableHead>
                <TableHead>Used by</TableHead>
                <TableHead>Used</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="w-10">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {codes.data?.codes.map((item) => (
                <TableRow key={item.id}>
                  <TableCell>
                    <code className="text-xs">{item.code}</code>
                  </TableCell>
                  <TableCell>{item.scope}</TableCell>
                  <TableCell>
                    <StatusBadge status={item.status} />
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {formatDate(item.expires_at)}
                  </TableCell>
                  <TableCell>
                    {item.used_by ? (
                      <div>
                        <div className="font-medium">
                          {item.used_by.display_name || item.used_by.email}
                        </div>
                        <div className="text-xs text-muted-foreground">{item.used_by.email}</div>
                      </div>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {item.used_at ? formatDate(item.used_at) : "—"}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {formatDate(item.created_at)}
                  </TableCell>
                  <TableCell>
                    {item.status === "available" && (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label={`Actions for ${item.code}`}
                          >
                            <MoreHorizontalIcon />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem
                            className="text-destructive"
                            disabled={revoke.isPending}
                            onSelect={() => revoke.mutate(item)}
                          >
                            Revoke code
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      {codes.data && codes.data.pagination.total_pages > 1 && (
        <div className="flex items-center justify-end gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={page <= 1}
            onClick={() => setPage((value) => value - 1)}
          >
            Previous
          </Button>
          <span className="text-sm text-muted-foreground">
            {page} / {codes.data.pagination.total_pages}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={page >= codes.data.pagination.total_pages}
            onClick={() => setPage((value) => value + 1)}
          >
            Next
          </Button>
        </div>
      )}

      <Dialog
        open={generateOpen}
        onOpenChange={(open) => (open ? setGenerateOpen(true) : closeGeneration())}
      >
        <DialogContent className="max-w-3xl">
          {generated ? (
            <GeneratedCodes codes={generated} onClose={closeGeneration} />
          ) : (
            <GenerateCodesForm
              scopes={codes.data?.scopes ?? ["registration"]}
              onGenerated={setGenerated}
            />
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function GenerateCodesForm({
  scopes,
  onGenerated,
}: {
  scopes: string[];
  onGenerated: (codes: AdminGeneratedCode[]) => void;
}) {
  const queryClient = useQueryClient();
  const [scope, setScope] = useState(scopes[0] ?? "registration");
  const [count, setCount] = useState(25);
  const [expiresInDays, setExpiresInDays] = useState(30);
  const [neverExpires, setNeverExpires] = useState(false);
  const generate = useMutation({
    mutationFn: () =>
      adminApi.generateCodes({
        scope,
        count,
        expires_in_days: neverExpires ? null : expiresInDays,
        never_expires: neverExpires,
      }),
    onSuccess: async (result) => {
      await queryClient.invalidateQueries({ queryKey: ["admin", "codes"] });
      onGenerated(result.codes);
    },
  });

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        generate.mutate();
      }}
    >
      <DialogHeader>
        <DialogTitle>Generate codes</DialogTitle>
        <DialogDescription>
          New values are available only until this dialog closes.
        </DialogDescription>
      </DialogHeader>
      <FieldGroup className="py-5">
        <Field>
          <FieldLabel htmlFor="code-scope">Scope</FieldLabel>
          <Select value={scope} onValueChange={setScope}>
            <SelectTrigger id="code-scope">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {scopes.map((item) => (
                <SelectItem key={item} value={item}>
                  {item}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="code-quantity">Quantity</FieldLabel>
            <Input
              id="code-quantity"
              type="number"
              min={1}
              max={500}
              value={count}
              onChange={(event) => setCount(Number(event.target.value))}
              required
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="code-expiration">Expires in days</FieldLabel>
            <Input
              id="code-expiration"
              type="number"
              min={1}
              max={3650}
              value={expiresInDays}
              disabled={neverExpires}
              onChange={(event) => setExpiresInDays(Number(event.target.value))}
              required={!neverExpires}
            />
          </Field>
        </div>
        <Field orientation="horizontal">
          <div>
            <FieldLabel htmlFor="code-never-expires">Never expires</FieldLabel>
            <FieldDescription>Leave these codes available until used or revoked.</FieldDescription>
          </div>
          <Switch
            id="code-never-expires"
            checked={neverExpires}
            onCheckedChange={setNeverExpires}
          />
        </Field>
      </FieldGroup>
      <DialogFooter>
        <Button type="submit" disabled={generate.isPending || count < 1 || count > 500}>
          {generate.isPending && <Spinner data-icon="inline-start" />}
          Generate
        </Button>
      </DialogFooter>
    </form>
  );
}

function GeneratedCodes({ codes, onClose }: { codes: AdminGeneratedCode[]; onClose: () => void }) {
  const [copied, setCopied] = useState(false);

  return (
    <>
      <DialogHeader>
        <DialogTitle>Generated codes</DialogTitle>
        <DialogDescription>Copy these values before closing this dialog.</DialogDescription>
      </DialogHeader>
      <div className="max-h-[55vh] overflow-auto border-y">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Code</TableHead>
              <TableHead>Scope</TableHead>
              <TableHead>Expires</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {codes.map((item) => (
              <TableRow key={item.id}>
                <TableCell>
                  <code className="break-all text-xs">{item.code}</code>
                </TableCell>
                <TableCell>{item.scope}</TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {formatDate(item.expires_at)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(codes.map((item) => item.code).join("\n"));
              setCopied(true);
            } catch (cause) {
              showErrorToast(cause);
            }
          }}
        >
          {copied ? <CheckIcon /> : <CopyIcon />}
          {copied ? "Copied" : "Copy all"}
        </Button>
        <Button type="button" onClick={onClose}>
          Done
        </Button>
      </DialogFooter>
    </>
  );
}
