import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";

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
import { Button } from "@/components/ui/button";
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { adminApi, type AdminRegistrationEmail } from "../admin.api";

const queryKey = ["admin", "registration", "emails"] as const;

function formatDate(value: string) {
  return new Date(value).toLocaleString();
}

function creatorLabel(entry: AdminRegistrationEmail) {
  return entry.created_by?.display_name || entry.created_by?.email || "Administrator";
}

export function RegistrationPanel() {
  const queryClient = useQueryClient();
  const [email, setEmail] = useState("");
  const [deleting, setDeleting] = useState<AdminRegistrationEmail | null>(null);
  const emails = useQuery({ queryKey, queryFn: adminApi.listRegistrationEmails });
  const add = useMutation({
    mutationFn: (input: { email: string }) => adminApi.addRegistrationEmail(input),
    onSuccess: async () => {
      setEmail("");
      await queryClient.invalidateQueries({ queryKey });
    },
  });
  const remove = useMutation({
    mutationFn: (entryId: string) => adminApi.removeRegistrationEmail(entryId),
    onSuccess: async () => {
      setDeleting(null);
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  return (
    <div className="flex flex-col gap-5">
      <form
        className="max-w-xl"
        onSubmit={(event) => {
          event.preventDefault();
          add.mutate({ email: email.trim() });
        }}
      >
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="registration-email">Email</FieldLabel>
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                id="registration-email"
                type="email"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                placeholder="name@example.com"
                required
              />
              <Button type="submit" disabled={add.isPending || email.trim().length === 0}>
                {add.isPending ? (
                  <Spinner data-icon="inline-start" />
                ) : (
                  <PlusIcon data-icon="inline-start" />
                )}
                Add email
              </Button>
            </div>
          </Field>
        </FieldGroup>
      </form>

      {emails.isLoading ? (
        <Skeleton className="h-48 w-full" aria-busy="true" />
      ) : emails.data?.emails.length === 0 ? (
        <Empty className="border-y">
          <EmptyHeader>
            <EmptyTitle>No authorized emails</EmptyTitle>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Email</TableHead>
                <TableHead>Added by</TableHead>
                <TableHead>Added</TableHead>
                <TableHead className="w-10">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {emails.data?.emails.map((entry) => (
                <TableRow key={entry.id}>
                  <TableCell className="font-medium">{entry.email}</TableCell>
                  <TableCell>{creatorLabel(entry)}</TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {formatDate(entry.created_at)}
                  </TableCell>
                  <TableCell>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={`Remove ${entry.email}`}
                      onClick={() => setDeleting(entry)}
                    >
                      <Trash2Icon />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <AlertDialog open={deleting !== null} onOpenChange={(open) => !open && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {deleting?.email}?</AlertDialogTitle>
            <AlertDialogDescription>
              This email will no longer authorize invite-only registration.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={remove.isPending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={remove.isPending}
              onClick={() => deleting && remove.mutate(deleting.id)}
            >
              {remove.isPending && <Spinner data-icon="inline-start" />}
              Remove email
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
