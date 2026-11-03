import * as RD from "@radix-ui/react-dialog";
import { BellOff, CheckCheck, Trash2, X } from "lucide-react";
import { useEffect } from "react";
import { Button, EmptyState, StatusDot } from "@/components/ui";
import { age, cn } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { Health } from "@/lib/types";
import { KINDS } from "@/features/resources/catalog";
import { useApp, type Notice } from "@/store/app";

const sevHealth: Record<Notice["severity"], Health> = { critical: "error", warning: "warning", info: "pending", success: "ok" };

export function NotificationCenter() {
  const open = useApp((s) => s.notificationsOpen);
  const setOpen = useApp((s) => s.setNotificationsOpen);
  const notices = useApp((s) => s.notices);
  const markRead = useApp((s) => s.markNoticesRead);
  const clear = useApp((s) => s.clearNotices);

  useEffect(() => {
    if (open) {
      const t = setTimeout(markRead, 1200);
      return () => clearTimeout(t);
    }
  }, [open, markRead]);

  const reveal = (n: Notice) => {
    const a = n.alert;
    if (!a?.objectKind || !a.objectName) return;
    const def = Object.values(KINDS).find((k) => k.ref.kind === a.objectKind);
    if (!def) return;
    const app = useApp.getState();
    app.setActiveCluster(a.clusterId);
    app.select({ clusterId: a.clusterId, resource: def.ref, namespace: a.namespace, name: a.objectName });
    setOpen(false);
  };

  return (
    <RD.Root open={open} onOpenChange={setOpen}>
      <RD.Portal>
        <RD.Overlay className="fixed inset-0 z-40 animate-fade-in bg-black/20" />
        <RD.Content className="fixed top-2 right-2 bottom-2 z-50 flex w-[400px] animate-slide-in flex-col rounded-xl border border-border bg-elevated shadow-pop focus:outline-none">
          <div className="flex h-12 items-center justify-between border-b border-border px-4">
            <RD.Title className="text-[14px] font-semibold">Notifications</RD.Title>
            <RD.Description className="sr-only">Cluster alerts and application messages</RD.Description>
            <div className="flex items-center gap-1">
              <Button size="icon-sm" variant="ghost" onClick={markRead} title="Mark all read">
                <CheckCheck className="size-4" />
              </Button>
              <Button
                size="icon-sm"
                variant="ghost"
                title="Clear all"
                onClick={() => {
                  clear();
                  api.clearAlerts().catch(() => {});
                }}
              >
                <Trash2 className="size-4" />
              </Button>
              <RD.Close asChild>
                <Button size="icon-sm" variant="ghost" aria-label="Close">
                  <X className="size-4" />
                </Button>
              </RD.Close>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            {notices.length === 0 ? (
              <EmptyState icon={<BellOff className="size-5" />} title="You're all caught up" description="Pod failures, deployment problems, node issues and connection changes appear here." />
            ) : (
              notices.map((n) => (
                <button
                  key={n.id}
                  onClick={() => reveal(n)}
                  className={cn("flex w-full gap-3 border-b border-border px-4 py-3 text-left hover:bg-hover", !n.read && "bg-accent-soft/40")}
                >
                  <StatusDot health={sevHealth[n.severity]} className="mt-1.5" />
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <div className="flex items-baseline justify-between gap-2">
                      <span className="truncate text-[13px] font-medium">{n.title}</span>
                      <span className="shrink-0 text-[11px] text-subtle">{age(n.ts)}</span>
                    </div>
                    {n.message && <span className="selectable line-clamp-3 text-[12px] text-muted">{n.message}</span>}
                    {n.clusterName && (
                      <span className="text-[11px] text-subtle">
                        {n.clusterName}
                        {n.alert?.namespace ? ` · ${n.alert.namespace}` : ""}
                      </span>
                    )}
                  </div>
                </button>
              ))
            )}
          </div>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed right-4 bottom-10 z-[60] flex w-96 flex-col gap-2">
      {toasts.map((t) => (
        <div key={t.id} className="pointer-events-auto flex animate-pop-in items-start gap-3 rounded-lg border border-border bg-elevated px-3.5 py-3 shadow-pop">
          <StatusDot health={t.kind === "success" ? "ok" : t.kind === "error" ? "error" : "pending"} className="mt-1.5" />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-[13px] font-medium">{t.title}</span>
            {t.message && <span className="selectable mt-0.5 line-clamp-4 text-[12px] break-words text-muted">{t.message}</span>}
          </div>
          <button className="text-subtle hover:text-fg" onClick={() => dismiss(t.id)} aria-label="Dismiss">
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
