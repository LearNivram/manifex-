import { AlertTriangle, Fingerprint, Gauge, Plug } from "lucide-react";
import { StatusDot } from "@/components/ui";
import { useActiveCluster, useApp } from "@/store/app";
import { statusHealth } from "./ClusterSwitcher";

export function StatusBar() {
  const active = useActiveCluster();
  const clusters = useApp((s) => s.clusters);
  const notices = useApp((s) => s.notices);
  const setNotificationsOpen = useApp((s) => s.setNotificationsOpen);
  const connected = clusters.filter((c) => c.status?.state === "connected").length;
  const critical = notices.filter((n) => !n.read && n.severity === "critical").length;
  const st = active?.status;
  return (
    <footer className="flex h-6 shrink-0 items-center gap-4 border-t border-border bg-sidebar px-3 text-[11.5px] text-muted">
      {active ? (
        <>
          <span className="flex items-center gap-1.5">
            <StatusDot health={statusHealth(active)} />
            <span className="font-medium text-fg">{active.profile.name}</span>
            <span>{st?.state ?? "disconnected"}</span>
          </span>
          {st?.message && st.state !== "connected" && <span className="max-w-96 truncate text-warn">{st.message}</span>}
          {st?.version && <span>{st.version}</span>}
          {st?.identity && (
            <span className="flex items-center gap-1" title="Authenticated identity">
              <Fingerprint className="size-3" /> {st.identity}
            </span>
          )}
          {st?.latencyMs != null && (
            <span className="flex items-center gap-1" title="API server round-trip">
              <Gauge className="size-3" /> {st.latencyMs} ms
            </span>
          )}
        </>
      ) : (
        <span>No cluster selected</span>
      )}
      <span className="flex-1" />
      {critical > 0 && (
        <button className="flex items-center gap-1 text-err hover:underline" onClick={() => setNotificationsOpen(true)}>
          <AlertTriangle className="size-3" /> {critical} critical
        </button>
      )}
      <span className="flex items-center gap-1">
        <Plug className="size-3" /> {connected}/{clusters.length} connected
      </span>
    </footer>
  );
}
