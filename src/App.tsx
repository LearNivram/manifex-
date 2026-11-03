import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import { Plug } from "lucide-react";
import { lazy, Suspense, useEffect } from "react";
import { Button, EmptyState, Spinner, TooltipProvider } from "@/components/ui";
import { ConfirmHost } from "@/components/ui/confirm";
import { activateCluster } from "@/components/layout/ClusterSwitcher";
import { CommandPalette } from "@/components/layout/CommandPalette";
import { NotificationCenter, Toasts } from "@/components/layout/NotificationCenter";
import { Sidebar } from "@/components/layout/Sidebar";
import { StatusBar } from "@/components/layout/StatusBar";
import { TitleBar } from "@/components/layout/TitleBar";
import { AccessView } from "@/features/access/AccessView";
import { ApplicationsView } from "@/features/apps/ApplicationsView";
import { AuditView } from "@/features/audit/AuditView";
import { AddClusterDialog } from "@/features/clusters/AddClusterDialog";
import { ClustersView } from "@/features/clusters/ClustersView";
import { CompareView } from "@/features/clusters/CompareView";
import { CustomResourcesView } from "@/features/custom/CustomResourcesView";
import { HelmView } from "@/features/helm/HelmView";
import { LogsView } from "@/features/logs/LogsView";
import { MonitoringView } from "@/features/monitoring/MonitoringView";
import { OverviewView } from "@/features/overview/OverviewView";
import { ActionDialogs } from "@/features/resources/actions";
import { KINDS, VIEW_KINDS } from "@/features/resources/catalog";
import { DetailPanel } from "@/features/resources/DetailPanel";
import { ResourceView } from "@/features/resources/ResourceView";
import { SettingsView } from "@/features/settings/SettingsView";
import { TerminalView } from "@/features/terminal/TerminalView";
import { useApplyTheme } from "@/hooks/useTheme";
import { parseDeepLink } from "@/lib/deeplink";
import { api, isTauri } from "@/lib/ipc";
import type { Alert, ClusterStatus } from "@/lib/types";
import { isConnected, useActiveCluster, useApp, VIEW_IDS, type ViewId } from "@/store/app";

const EditorPanel = lazy(() => import("@/features/editor/EditorPanel"));

function NotConnected() {
  const active = useActiveCluster();
  const navigate = useApp((s) => s.navigate);
  return (
    <EmptyState
      icon={<Plug className="size-5" />}
      title={active ? `${active.profile.name} is not connected` : "No cluster selected"}
      description={active?.status?.message ?? "Select a cluster in the sidebar or manage your connections."}
      action={
        <div className="flex gap-2">
          {active && (
            <Button variant="primary" onClick={() => activateCluster(active.profile.id)}>
              Connect
            </Button>
          )}
          <Button onClick={() => navigate("clusters")}>Manage clusters</Button>
        </div>
      }
    />
  );
}

function MainView() {
  const view = useApp((s) => s.view);
  const active = useActiveCluster();
  const clusterViews: ViewId[] = ["clusters", "settings", "audit", "compare", "terminal"];
  if (!clusterViews.includes(view) && !isConnected(active)) return <NotConnected />;
  const id = active?.profile.id ?? "";
  switch (view) {
    case "clusters":
      return <ClustersView />;
    case "settings":
      return <SettingsView />;
    case "audit":
      return <AuditView />;
    case "compare":
      return <CompareView />;
    case "overview":
      return <OverviewView key={id} />;
    case "applications":
      return <ApplicationsView key={id} />;
    case "monitoring":
      return <MonitoringView key={id} />;
    case "helm":
      return <HelmView key={id} />;
    case "logs":
      return <LogsView key={id} />;
    case "access":
      return <AccessView key={id} />;
    case "custom":
      return <CustomResourcesView key={id} />;
    case "events":
      return <ResourceView key={`${id}-events`} clusterId={id} title="Events" kindKeys={["events"]} description="Live cluster events. Warning events also feed alerts and notifications." />;
    case "pods":
      return <ResourceView key={`${id}-pods`} clusterId={id} title="Pods" kindKeys={["pods"]} />;
    case "deployments":
      return <ResourceView key={`${id}-deployments`} clusterId={id} title="Deployments" kindKeys={["deployments"]} />;
    case "services":
      return <ResourceView key={`${id}-services`} clusterId={id} title="Services" kindKeys={["services", "endpointslices"]} />;
    case "nodes":
      return <ResourceView key={`${id}-nodes`} clusterId={id} title="Nodes" kindKeys={["nodes"]} />;
    case "workloads":
      return <ResourceView key={`${id}-workloads`} clusterId={id} title="Workloads" kindKeys={VIEW_KINDS.workloads} />;
    case "networking":
      return <ResourceView key={`${id}-networking`} clusterId={id} title="Networking" kindKeys={VIEW_KINDS.networking} description="Services, ingress, Gateway API and network policies." />;
    case "storage":
      return <ResourceView key={`${id}-storage`} clusterId={id} title="Storage" kindKeys={VIEW_KINDS.storage} />;
    case "config":
      return <ResourceView key={`${id}-config`} clusterId={id} title="Configuration" kindKeys={VIEW_KINDS.config} />;
    default:
      return null;
  }
}

async function handleDeepLinks(urls: string[]) {
  for (const raw of urls) {
    const t = parseDeepLink(raw);
    if (!t) continue;
    const app = useApp.getState();
    if (t.clusterId && app.clusters.some((c) => c.profile.id === t.clusterId)) {
      await activateCluster(t.clusterId);
      if (t.namespace) app.setNamespace(t.clusterId, t.namespace);
      const def = t.kind ? KINDS[t.kind] : undefined;
      if (def && t.name) app.select({ clusterId: t.clusterId, resource: def.ref, namespace: def.ref.namespaced ? t.namespace : null, name: t.name });
    }
    if (t.view) app.navigate(t.view);
  }
}

function handleMenuAction(action: string) {
  const app = useApp.getState();
  switch (action) {
    case "command-palette":
      return app.setPalette(true, "commands");
    case "search":
      return app.setPalette(true, "search");
    case "toggle-sidebar":
      return app.toggleSidebar();
    case "toggle-details":
      return app.select(null);
    case "settings":
    case "check-updates":
      return app.navigate("settings");
    case "new-window":
      return api.openWindow(app.activeClusterId ?? undefined).catch(() => {});
    case "add-cluster":
      return app.setAddClusterOpen(true);
    case "terminal":
      return app.navigate("terminal");
    case "new-resource":
      if (app.activeClusterId) app.openEditor({ mode: "create", clusterId: app.activeClusterId, namespace: app.namespaces[app.activeClusterId], initial: "" });
      return;
  }
}

/** Wires native events (cluster status, alerts, menus, tray, deep links) into the store. */
function useBootstrap() {
  useEffect(() => {
    if (!isTauri) return;
    const app = useApp.getState();
    const params = new URLSearchParams(window.location.search);
    const unlisten: Promise<() => void>[] = [];

    (async () => {
      const [settings, clusters] = await Promise.all([api.getSettings(), api.listClusters()]);
      app.setSettings(settings);
      app.setClusters(clusters);
      const initialView = params.get("view");
      if (initialView && VIEW_IDS.includes(initialView as ViewId)) app.navigate(initialView as ViewId);
      let last: string | null = params.get("cluster");
      if (!last) {
        try {
          last = localStorage.getItem("manifex.activeCluster");
        } catch {
          last = null;
        }
      }
      // show the window as soon as the first state is rendered
      requestAnimationFrame(() => getCurrentWindow().show().catch(() => {}));
      if (last && clusters.some((c) => c.profile.id === last)) {
        await activateCluster(last);
        if (useApp.getState().view === "clusters" && !initialView) app.navigate("overview");
      }
      // historic alerts (e.g. raised while this window was closed)
      api.listAlerts().then((alerts) =>
        alerts
          .slice()
          .reverse()
          .forEach((a) => useApp.getState().pushNotice({ severity: a.severity, title: a.title, message: a.message, clusterId: a.clusterId, clusterName: a.clusterName, alert: a, ts: a.ts })),
      );
      handleDeepLinks(await api.takeDeepLinks().catch(() => []));
      try {
        if (!(await isPermissionGranted())) await requestPermission();
      } catch {
        /* notifications unavailable */
      }
      if (settings.autoCheckUpdates && getCurrentWindow().label === "main") {
        import("@tauri-apps/plugin-updater")
          .then(({ check }) => check())
          .then((u) => u && useApp.getState().pushNotice({ severity: "info", title: `Manifex ${u.version} is available`, message: "Open Settings → Updates to install." }))
          .catch(() => {});
      }
    })().catch((e) => app.toast({ kind: "error", title: "Startup failed", message: String(e?.message ?? e) }));

    unlisten.push(
      listen<ClusterStatus>("cluster://status", async (e) => {
        const s = useApp.getState();
        if (!s.clusters.some((c) => c.profile.id === e.payload.id)) s.setClusters(await api.listClusters());
        useApp.getState().updateStatus(e.payload);
      }),
    );
    unlisten.push(
      listen<Alert>("alerts://new", (e) => {
        const a = e.payload;
        useApp.getState().pushNotice({ severity: a.severity, title: a.title, message: a.message, clusterId: a.clusterId, clusterName: a.clusterName, alert: a, ts: a.ts });
      }),
    );
    unlisten.push(
      listen<{ severity: string; title: string; body: string; clusterId?: string }>("notify://message", (e) => {
        const p = e.payload;
        const name = useApp.getState().clusters.find((c) => c.profile.id === p.clusterId)?.profile.name;
        useApp.getState().pushNotice({ severity: p.severity === "critical" ? "critical" : "info", title: p.title, message: p.body, clusterId: p.clusterId, clusterName: name });
      }),
    );
    unlisten.push(listen<string>("menu://action", (e) => document.hasFocus() && handleMenuAction(e.payload)));
    unlisten.push(listen<string>("tray://open-cluster", (e) => activateCluster(e.payload)));
    unlisten.push(listen<string[]>("deeplink://open", () => api.takeDeepLinks().then(handleDeepLinks).catch(() => {})));
    return () => unlisten.forEach((p) => p.then((f) => f()));
  }, []);

  // Keyboard shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const app = useApp.getState();
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        app.setPalette(!app.paletteOpen, "commands");
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        app.setPalette(true, "search");
      } else if (mod && e.key.toLowerCase() === "b" && !app.editor) {
        e.preventDefault();
        app.toggleSidebar();
      } else if (mod && e.key === ",") {
        e.preventDefault();
        app.navigate("settings");
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        handleMenuAction("new-window");
      } else if (mod && !e.shiftKey && e.key.toLowerCase() === "n" && !app.editor) {
        e.preventDefault();
        handleMenuAction("new-resource");
      } else if (mod && /^[1-9]$/.test(e.key) && !app.editor) {
        const c = app.clusters[Number(e.key) - 1];
        if (c) {
          e.preventDefault();
          activateCluster(c.profile.id);
        }
      } else if (e.key === "Escape" && app.selection && !app.paletteOpen && !app.editor && !document.querySelector("[role=dialog]")) {
        app.select(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

export default function App() {
  useApplyTheme();
  useBootstrap();
  const view = useApp((s) => s.view);
  const editor = useApp((s) => s.editor);
  const selection = useApp((s) => s.selection);
  const activeId = useApp((s) => s.activeClusterId);

  if (!isTauri) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState title="Manifex desktop" description="This interface runs inside the Manifex desktop application (Tauri). Start it with `npm run app:dev`." />
      </div>
    );
  }

  return (
    <TooltipProvider>
      <div className="flex h-full flex-col">
        <TitleBar />
        <div className="flex min-h-0 flex-1">
          <Sidebar />
          <main className="relative flex min-w-0 flex-1">
            <div className="min-w-0 flex-1">
              {/* The terminal stays mounted so sessions survive navigation. */}
              <div className={view === "terminal" ? "h-full" : "hidden"}>
                <TerminalView />
              </div>
              {view !== "terminal" && <MainView />}
            </div>
            {selection && selection.clusterId === activeId && <DetailPanel />}
          </main>
        </div>
        <StatusBar />
      </div>
      <CommandPalette />
      <NotificationCenter />
      <AddClusterDialog />
      <ActionDialogs />
      <ConfirmHost />
      <Toasts />
      {editor && (
        <Suspense
          fallback={
            <div className="fixed inset-0 z-30 flex items-center justify-center bg-canvas">
              <Spinner />
            </div>
          }
        >
          <EditorPanel state={editor} />
        </Suspense>
      )}
    </TooltipProvider>
  );
}
