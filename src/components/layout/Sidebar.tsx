import {
  Activity,
  AppWindow,
  Boxes,
  Cable,
  CircleGauge,
  Container,
  Database,
  FileCog,
  FileClock,
  Layers,
  LayoutDashboard,
  ListTree,
  Network,
  PanelLeftClose,
  PanelLeftOpen,
  Puzzle,
  Rocket,
  ScrollText,
  Server,
  Settings,
  ShieldCheck,
  Ship,
  SquareTerminal,
  Waypoints,
} from "lucide-react";
import React, { useMemo } from "react";
import { Tooltip } from "@/components/ui";
import { useWatch } from "@/hooks/useWatch";
import { cn } from "@/lib/format";
import { KINDS } from "@/features/resources/catalog";
import { isConnected, useActiveCluster, useApp, useNamespace, type ViewId } from "@/store/app";
import { ClusterSwitcher } from "./ClusterSwitcher";

interface NavItem {
  id: ViewId;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  needsCluster?: boolean;
}

export const NAV: { title?: string; items: NavItem[] }[] = [
  {
    items: [
      { id: "overview", label: "Overview", icon: LayoutDashboard, needsCluster: true },
      { id: "applications", label: "Applications", icon: AppWindow, needsCluster: true },
      { id: "monitoring", label: "Monitoring", icon: CircleGauge, needsCluster: true },
      { id: "events", label: "Events", icon: Activity, needsCluster: true },
    ],
  },
  {
    title: "Workloads",
    items: [
      { id: "workloads", label: "Workloads", icon: Layers, needsCluster: true },
      { id: "pods", label: "Pods", icon: Container, needsCluster: true },
      { id: "deployments", label: "Deployments", icon: Rocket, needsCluster: true },
    ],
  },
  {
    title: "Network & data",
    items: [
      { id: "services", label: "Services", icon: Waypoints, needsCluster: true },
      { id: "networking", label: "Networking", icon: Network, needsCluster: true },
      { id: "storage", label: "Storage", icon: Database, needsCluster: true },
      { id: "config", label: "Configuration", icon: FileCog, needsCluster: true },
    ],
  },
  {
    title: "Operate",
    items: [
      { id: "helm", label: "Helm", icon: Ship, needsCluster: true },
      { id: "logs", label: "Logs", icon: ScrollText, needsCluster: true },
      { id: "terminal", label: "Terminal", icon: SquareTerminal },
    ],
  },
  {
    title: "Administration",
    items: [
      { id: "nodes", label: "Nodes", icon: Server, needsCluster: true },
      { id: "access", label: "Access control", icon: ShieldCheck, needsCluster: true },
      { id: "custom", label: "Custom resources", icon: Puzzle, needsCluster: true },
      { id: "audit", label: "Audit log", icon: FileClock },
    ],
  },
];

export const BOTTOM_NAV: NavItem[] = [
  { id: "clusters", label: "Clusters", icon: Boxes },
  { id: "settings", label: "Settings", icon: Settings },
];

function NavButton({ item, active, collapsed, disabled, onClick }: { item: NavItem; active: boolean; collapsed: boolean; disabled: boolean; onClick: () => void }) {
  const Icon = item.icon;
  const btn = (
    <button
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] transition-colors",
        active ? "bg-active font-medium text-fg" : "text-muted hover:bg-hover hover:text-fg",
        disabled && "pointer-events-none opacity-40",
        collapsed && "justify-center px-0",
      )}
      aria-current={active ? "page" : undefined}
    >
      <Icon className={cn("size-4 shrink-0", active && "text-accent")} />
      {!collapsed && <span className="truncate">{item.label}</span>}
    </button>
  );
  return collapsed ? (
    <Tooltip content={item.label} side="right">
      {btn}
    </Tooltip>
  ) : (
    btn
  );
}

function NamespacePicker() {
  const active = useActiveCluster();
  const ns = useNamespace();
  const setNamespace = useApp((s) => s.setNamespace);
  const { rows } = useWatch(active?.profile.id, KINDS.namespaces.ref, null, { enabled: isConnected(active) });
  const names = useMemo(() => rows.map((r) => r.name).sort(), [rows]);
  if (!active || !isConnected(active)) return null;
  return (
    <div className="flex items-center gap-2 px-1">
      <ListTree className="size-3.5 shrink-0 text-subtle" />
      <select
        aria-label="Namespace"
        value={ns}
        onChange={(e) => setNamespace(active.profile.id, e.target.value)}
        className="h-7 min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1 text-[12.5px] text-fg hover:border-border focus:border-accent focus:outline-none"
      >
        <option value="">All namespaces</option>
        {ns && !names.includes(ns) && <option value={ns}>{ns}</option>}
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
    </div>
  );
}

export function Sidebar() {
  const view = useApp((s) => s.view);
  const navigate = useApp((s) => s.navigate);
  const collapsed = useApp((s) => s.sidebarCollapsed);
  const toggle = useApp((s) => s.toggleSidebar);
  const active = useActiveCluster();
  const connected = isConnected(active);

  return (
    <nav className={cn("flex shrink-0 flex-col border-r border-border bg-sidebar transition-[width] duration-150", collapsed ? "w-14" : "w-60")}>
      <div className="flex flex-col gap-2 p-2">
        <ClusterSwitcher collapsed={collapsed} />
        {!collapsed && <NamespacePicker />}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {NAV.map((section, i) => (
          <div key={i} className="mt-2 flex flex-col gap-0.5">
            {section.title && !collapsed && <div className="px-2.5 pt-2 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase">{section.title}</div>}
            {section.title && collapsed && <div className="mx-3 my-1.5 h-px bg-border" />}
            {section.items.map((item) => (
              <NavButton key={item.id} item={item} active={view === item.id} collapsed={collapsed} disabled={!!item.needsCluster && !connected} onClick={() => navigate(item.id)} />
            ))}
          </div>
        ))}
      </div>
      <div className="flex flex-col gap-0.5 border-t border-border p-2">
        {BOTTOM_NAV.map((item) => (
          <NavButton key={item.id} item={item} active={view === item.id} collapsed={collapsed} disabled={false} onClick={() => navigate(item.id)} />
        ))}
        <button onClick={toggle} className={cn("flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[12.5px] text-subtle hover:bg-hover hover:text-fg", collapsed && "justify-center px-0")} aria-label="Toggle sidebar">
          {collapsed ? <PanelLeftOpen className="size-4" /> : <PanelLeftClose className="size-4" />}
          {!collapsed && <span>Collapse</span>}
        </button>
      </div>
    </nav>
  );
}

export { Cable };
