import { create } from "zustand";
import type { Alert, AppSettings, ClusterStatus, ClusterView, ResourceRef } from "@/lib/types";

export type ViewId =
  | "overview"
  | "clusters"
  | "nodes"
  | "workloads"
  | "pods"
  | "deployments"
  | "services"
  | "networking"
  | "storage"
  | "config"
  | "helm"
  | "monitoring"
  | "logs"
  | "events"
  | "terminal"
  | "applications"
  | "access"
  | "custom"
  | "audit"
  | "compare"
  | "settings";

export const VIEW_IDS: ViewId[] = [
  "overview",
  "clusters",
  "nodes",
  "workloads",
  "pods",
  "deployments",
  "services",
  "networking",
  "storage",
  "config",
  "helm",
  "monitoring",
  "logs",
  "events",
  "terminal",
  "applications",
  "access",
  "custom",
  "audit",
  "compare",
  "settings",
];

export interface Selection {
  clusterId: string;
  resource: ResourceRef;
  namespace?: string | null;
  name: string;
  tab?: string;
}

export interface TerminalTab {
  id: string;
  clusterId: string;
  namespace: string;
  pod: string;
  container?: string;
  shell: string;
}

export interface LogTarget {
  clusterId: string;
  namespace: string;
  pods: string[];
  container?: string;
  title: string;
}

export interface Notice {
  id: string;
  ts: string;
  severity: "info" | "warning" | "critical" | "success";
  title: string;
  message?: string;
  clusterId?: string | null;
  clusterName?: string;
  alert?: Alert;
  read: boolean;
}

export interface Toast {
  id: string;
  kind: "success" | "error" | "info";
  title: string;
  message?: string;
}

export type EditorState =
  | { mode: "create"; clusterId: string; namespace?: string | null; initial?: string }
  | { mode: "edit"; clusterId: string; resource: ResourceRef; namespace?: string | null; name: string };

interface AppStore {
  clusters: ClusterView[];
  activeClusterId: string | null;
  namespaces: Record<string, string>;
  view: ViewId;
  viewParams: Record<string, string>;
  selection: Selection | null;
  sidebarCollapsed: boolean;
  paletteOpen: boolean;
  paletteMode: "commands" | "search";
  addClusterOpen: boolean;
  notificationsOpen: boolean;
  notices: Notice[];
  toasts: Toast[];
  settings: AppSettings | null;
  terminals: TerminalTab[];
  activeTerminal: string | null;
  logTarget: LogTarget | null;
  editor: EditorState | null;

  setClusters: (c: ClusterView[]) => void;
  updateStatus: (s: ClusterStatus) => void;
  setActiveCluster: (id: string | null) => void;
  setNamespace: (clusterId: string, ns: string) => void;
  navigate: (view: ViewId, params?: Record<string, string>) => void;
  select: (s: Selection | null) => void;
  toggleSidebar: () => void;
  setPalette: (open: boolean, mode?: "commands" | "search") => void;
  setAddClusterOpen: (open: boolean) => void;
  setNotificationsOpen: (open: boolean) => void;
  pushNotice: (n: Omit<Notice, "id" | "ts" | "read"> & { ts?: string }) => void;
  markNoticesRead: () => void;
  clearNotices: () => void;
  toast: (t: Omit<Toast, "id">) => void;
  dismissToast: (id: string) => void;
  setSettings: (s: AppSettings) => void;
  openTerminal: (t: Omit<TerminalTab, "id">) => void;
  closeTerminal: (id: string) => void;
  setActiveTerminal: (id: string) => void;
  openLogs: (t: LogTarget) => void;
  openEditor: (e: EditorState | null) => void;
}

let seq = 0;
const nextId = () => `${Date.now().toString(36)}-${(seq++).toString(36)}`;

export const useApp = create<AppStore>((set, get) => ({
  clusters: [],
  activeClusterId: null,
  namespaces: {},
  view: "clusters",
  viewParams: {},
  selection: null,
  sidebarCollapsed: false,
  paletteOpen: false,
  paletteMode: "commands",
  addClusterOpen: false,
  notificationsOpen: false,
  notices: [],
  toasts: [],
  settings: null,
  terminals: [],
  activeTerminal: null,
  logTarget: null,
  editor: null,

  setClusters: (clusters) => set({ clusters }),
  updateStatus: (s) =>
    set({
      clusters: get().clusters.map((c) => (c.profile.id === s.id ? { ...c, status: s } : c)),
    }),
  setActiveCluster: (id) => {
    if (id === get().activeClusterId) return;
    set({ activeClusterId: id, selection: null });
    try {
      if (id) localStorage.setItem("manifex.activeCluster", id);
    } catch {
      /* storage unavailable */
    }
  },
  setNamespace: (clusterId, ns) => set({ namespaces: { ...get().namespaces, [clusterId]: ns }, selection: null }),
  navigate: (view, params = {}) => set({ view, viewParams: params }),
  select: (selection) => set({ selection }),
  toggleSidebar: () => set({ sidebarCollapsed: !get().sidebarCollapsed }),
  setPalette: (paletteOpen, mode = "commands") => set({ paletteOpen, paletteMode: mode }),
  setAddClusterOpen: (addClusterOpen) => set({ addClusterOpen }),
  setNotificationsOpen: (notificationsOpen) => set({ notificationsOpen }),
  pushNotice: (n) =>
    set({ notices: [{ ...n, id: nextId(), ts: n.ts ?? new Date().toISOString(), read: false }, ...get().notices].slice(0, 300) }),
  markNoticesRead: () => set({ notices: get().notices.map((n) => ({ ...n, read: true })) }),
  clearNotices: () => set({ notices: [] }),
  toast: (t) => {
    const id = nextId();
    set({ toasts: [...get().toasts, { ...t, id }].slice(-4) });
    setTimeout(() => get().dismissToast(id), t.kind === "error" ? 8000 : 4000);
  },
  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
  setSettings: (settings) => set({ settings }),
  openTerminal: (t) => {
    const id = nextId();
    set({ terminals: [...get().terminals, { ...t, id }], activeTerminal: id, view: "terminal" });
  },
  closeTerminal: (id) => {
    const terminals = get().terminals.filter((t) => t.id !== id);
    set({ terminals, activeTerminal: get().activeTerminal === id ? terminals.at(-1)?.id ?? null : get().activeTerminal });
  },
  setActiveTerminal: (activeTerminal) => set({ activeTerminal }),
  openLogs: (logTarget) => set({ logTarget, view: "logs" }),
  openEditor: (editor) => set({ editor }),
}));

export function useActiveCluster(): ClusterView | null {
  return useApp((s) => s.clusters.find((c) => c.profile.id === s.activeClusterId) ?? null);
}

export function useNamespace(): string {
  return useApp((s) => (s.activeClusterId ? s.namespaces[s.activeClusterId] ?? "" : ""));
}

export function isConnected(c?: ClusterView | null): boolean {
  return c?.status?.state === "connected" || c?.status?.state === "reconnecting";
}
