import { Bell, ExternalLink, Search } from "lucide-react";
import { Button, Kbd, Tooltip } from "@/components/ui";
import { cn } from "@/lib/format";
import { api } from "@/lib/ipc";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";
import { clusterColor, statusHealth } from "./ClusterSwitcher";
import { StatusDot } from "@/components/ui";

const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);
export const MOD = isMac ? "⌘" : "Ctrl";

/**
 * Top bar. The active cluster is always visible here – with its environment
 * colour – so actions are never performed against the wrong cluster by accident.
 */
export function TitleBar() {
  const active = useActiveCluster();
  const ns = useNamespace();
  const setPalette = useApp((s) => s.setPalette);
  const unread = useApp((s) => s.notices.filter((n) => !n.read).length);
  const setNotificationsOpen = useApp((s) => s.setNotificationsOpen);
  const prod = active?.profile.environment === "production";

  return (
    <header className="relative flex h-11 shrink-0 items-center gap-3 border-b border-border bg-panel pr-3" data-tauri-drag-region style={{ paddingLeft: isMac ? 84 : 12 }}>
      {active && <div className="absolute inset-x-0 top-0 h-[2px]" style={{ background: clusterColor(active) }} />}
      <div className="flex min-w-0 items-center gap-2" data-tauri-drag-region>
        <span className="text-[13px] font-semibold tracking-tight">Manifex</span>
        {active && (
          <>
            <span className="text-subtle">/</span>
            <span
              className={cn("flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-[12.5px] font-medium", prod ? "border-err/40 bg-err-soft text-err" : "border-border")}
              title={active.status?.server ?? undefined}
            >
              <StatusDot health={statusHealth(active)} pulse={active.status?.state === "reconnecting"} />
              {active.profile.name}
              {prod && <span className="text-[10.5px] font-bold tracking-wide uppercase">production</span>}
            </span>
            <span className="truncate text-[12.5px] text-muted">{ns ? `namespace ${ns}` : "all namespaces"}</span>
          </>
        )}
      </div>
      <div className="flex flex-1 justify-center" data-tauri-drag-region>
        <button
          onClick={() => setPalette(true, "search")}
          className="flex h-7 w-full max-w-md items-center gap-2 rounded-md border border-border bg-canvas px-2.5 text-[12.5px] text-subtle transition-colors hover:border-border-strong"
        >
          <Search className="size-3.5" />
          <span className="flex-1 text-left">Search resources across clusters, run commands…</span>
          <Kbd>{MOD}</Kbd>
          <Kbd>K</Kbd>
        </button>
      </div>
      <div className="flex items-center gap-1">
        <Tooltip content="Open new window">
          <Button variant="ghost" size="icon" onClick={() => api.openWindow(active?.profile.id).catch(() => {})}>
            <ExternalLink className="size-4" />
          </Button>
        </Tooltip>
        <Tooltip content="Notifications">
          <Button variant="ghost" size="icon" className="relative" onClick={() => setNotificationsOpen(true)}>
            <Bell className="size-4" />
            {unread > 0 && (
              <span className="tabular absolute top-1 right-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-err px-0.5 text-[9.5px] font-bold text-white">
                {unread > 99 ? "99+" : unread}
              </span>
            )}
          </Button>
        </Tooltip>
      </div>
    </header>
  );
}
