// Resource catalog: how each Kubernetes kind is addressed, displayed and which
// actions it supports.

import React from "react";
import { StatusText } from "@/components/ui";
import { age, bytes, cpu } from "@/lib/format";
import type { ApiResourceInfo, ResourceRef, Row } from "@/lib/types";

export interface Column {
  id: string;
  title: string;
  width?: number; // fr weight
  minWidth?: number;
  align?: "left" | "right";
  render: (row: Row) => React.ReactNode;
  sort?: (row: Row) => string | number;
  mono?: boolean;
}

export type ActionId =
  | "logs"
  | "shell"
  | "scale"
  | "restart"
  | "pause"
  | "history"
  | "cordon"
  | "drain"
  | "trigger"
  | "suspend"
  | "edit"
  | "delete";

export interface KindDef {
  key: string;
  ref: ResourceRef;
  title: string;
  /** Group/kind is only shown when the API exists in the cluster (e.g. Gateway API). */
  optional?: boolean;
  columns: Column[];
  actions: ActionId[];
}

const K = (group: string, version: string, kind: string, plural: string, namespaced: boolean): ResourceRef => ({
  group,
  version,
  kind,
  plural,
  namespaced,
});

const c = (row: Row, key: string) => row.cells[key];
const text = (v: unknown) => (v === null || v === undefined || v === "" ? <span className="text-subtle">—</span> : String(v));

export const nameCol: Column = {
  id: "name",
  title: "Name",
  width: 3,
  minWidth: 200,
  render: (r) => <span className="truncate font-medium text-fg">{r.name}</span>,
  sort: (r) => r.name,
};
export const nsCol: Column = { id: "namespace", title: "Namespace", width: 1.4, minWidth: 110, render: (r) => text(r.namespace), sort: (r) => r.namespace ?? "" };
export const ageCol: Column = {
  id: "age",
  title: "Age",
  width: 0.6,
  minWidth: 64,
  align: "right",
  render: (r) => <span className="tabular text-muted">{age(r.created)}</span>,
  sort: (r) => -(Date.parse(r.created ?? "") || 0),
};
export const statusCol: Column = {
  id: "status",
  title: "Status",
  width: 1.4,
  minWidth: 120,
  render: (r) => (r.status ? <StatusText health={r.health}>{r.status}</StatusText> : <span className="text-subtle">—</span>),
  sort: (r) => `${["error", "warning", "pending", "ok", "neutral"].indexOf(r.health)}${r.status}`,
};
const cell = (id: string, title: string, opts: Partial<Column> = {}): Column => ({
  id,
  title,
  width: 1,
  minWidth: 80,
  render: (r) => text(c(r, id)),
  sort: (r) => {
    const v = c(r, id);
    return typeof v === "number" ? v : String(v ?? "");
  },
  ...opts,
});
const images: Column = {
  id: "images",
  title: "Images",
  width: 2.2,
  minWidth: 160,
  render: (r) => <span className="truncate font-mono text-[12px] text-muted">{(c(r, "images") as string[] | undefined)?.join(", ") || "—"}</span>,
};

const WORKLOAD_ACTIONS: ActionId[] = ["logs", "scale", "restart", "edit", "delete"];

export const KINDS: Record<string, KindDef> = {
  pods: {
    key: "pods",
    ref: K("", "v1", "Pod", "pods", true),
    title: "Pods",
    columns: [
      nameCol,
      nsCol,
      cell("ready", "Ready", { width: 0.6, minWidth: 64, render: (r) => <span className="tabular">{c(r, "ready")}</span> }),
      statusCol,
      cell("restarts", "Restarts", {
        width: 0.7,
        minWidth: 72,
        align: "right",
        render: (r) => <span className={`tabular ${c(r, "restarts") > 0 ? "text-warn" : "text-muted"}`}>{c(r, "restarts")}</span>,
      }),
      cell("node", "Node", { width: 1.4 }),
      cell("ip", "IP", { width: 1, mono: true }),
      ageCol,
    ],
    actions: ["logs", "shell", "edit", "delete"],
  },
  deployments: {
    key: "deployments",
    ref: K("apps", "v1", "Deployment", "deployments", true),
    title: "Deployments",
    columns: [
      nameCol,
      nsCol,
      cell("ready", "Ready", { width: 0.6, render: (r) => <span className="tabular">{c(r, "ready")}</span> }),
      cell("upToDate", "Up-to-date", { width: 0.7, align: "right" }),
      cell("available", "Available", { width: 0.7, align: "right" }),
      statusCol,
      images,
      ageCol,
    ],
    actions: ["logs", "scale", "restart", "pause", "history", "edit", "delete"],
  },
  statefulsets: {
    key: "statefulsets",
    ref: K("apps", "v1", "StatefulSet", "statefulsets", true),
    title: "StatefulSets",
    columns: [nameCol, nsCol, cell("ready", "Ready", { width: 0.6 }), statusCol, cell("serviceName", "Service"), images, ageCol],
    actions: WORKLOAD_ACTIONS,
  },
  daemonsets: {
    key: "daemonsets",
    ref: K("apps", "v1", "DaemonSet", "daemonsets", true),
    title: "DaemonSets",
    columns: [nameCol, nsCol, cell("desired", "Desired", { width: 0.6, align: "right" }), cell("ready", "Ready", { width: 0.6 }), cell("upToDate", "Up-to-date", { width: 0.7, align: "right" }), statusCol, cell("nodeSelector", "Node selector", { width: 1.4 }), ageCol],
    actions: ["logs", "restart", "edit", "delete"],
  },
  replicasets: {
    key: "replicasets",
    ref: K("apps", "v1", "ReplicaSet", "replicasets", true),
    title: "ReplicaSets",
    columns: [nameCol, nsCol, cell("desired", "Desired", { width: 0.6, align: "right" }), cell("ready", "Ready", { width: 0.6 }), cell("revision", "Revision", { width: 0.6, align: "right" }), cell("ownerName", "Owner", { width: 1.4 }), ageCol],
    actions: ["scale", "edit", "delete"],
  },
  jobs: {
    key: "jobs",
    ref: K("batch", "v1", "Job", "jobs", true),
    title: "Jobs",
    columns: [nameCol, nsCol, cell("completions", "Completions", { width: 0.8 }), statusCol, cell("ownerName", "Owner", { width: 1.2 }), ageCol],
    actions: ["logs", "edit", "delete"],
  },
  cronjobs: {
    key: "cronjobs",
    ref: K("batch", "v1", "CronJob", "cronjobs", true),
    title: "CronJobs",
    columns: [
      nameCol,
      nsCol,
      cell("schedule", "Schedule", { mono: true, render: (r) => <span className="font-mono text-[12px]">{c(r, "schedule")}</span> }),
      statusCol,
      cell("active", "Active", { width: 0.5, align: "right" }),
      cell("lastSchedule", "Last schedule", { render: (r) => <span className="tabular text-muted">{c(r, "lastSchedule") ? `${age(c(r, "lastSchedule"))} ago` : "never"}</span> }),
      ageCol,
    ],
    actions: ["trigger", "suspend", "edit", "delete"],
  },
  hpas: {
    key: "hpas",
    ref: K("autoscaling", "v2", "HorizontalPodAutoscaler", "horizontalpodautoscalers", true),
    title: "Autoscalers",
    columns: [nameCol, nsCol, cell("reference", "Target", { width: 1.6 }), cell("minReplicas", "Min", { width: 0.5, align: "right" }), cell("maxReplicas", "Max", { width: 0.5, align: "right" }), cell("replicas", "Replicas", { width: 0.6, align: "right" }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  pdbs: {
    key: "pdbs",
    ref: K("policy", "v1", "PodDisruptionBudget", "poddisruptionbudgets", true),
    title: "Disruption budgets",
    columns: [nameCol, nsCol, cell("minAvailable", "Min available", { width: 0.8 }), cell("maxUnavailable", "Max unavailable", { width: 0.8 }), cell("allowedDisruptions", "Allowed", { width: 0.6, align: "right" }), ageCol],
    actions: ["edit", "delete"],
  },
  services: {
    key: "services",
    ref: K("", "v1", "Service", "services", true),
    title: "Services",
    columns: [nameCol, nsCol, cell("type", "Type", { width: 0.9 }), cell("clusterIP", "Cluster IP", { mono: true }), cell("externalIP", "External", { width: 1.2 }), cell("ports", "Ports", { width: 1.6, mono: true }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  ingresses: {
    key: "ingresses",
    ref: K("networking.k8s.io", "v1", "Ingress", "ingresses", true),
    title: "Ingresses",
    columns: [nameCol, nsCol, cell("class", "Class"), cell("hosts", "Hosts", { width: 2 }), cell("address", "Address", { width: 1.2 }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  ingressclasses: {
    key: "ingressclasses",
    ref: K("networking.k8s.io", "v1", "IngressClass", "ingressclasses", false),
    title: "Ingress classes",
    columns: [nameCol, cell("controller", "Controller", { width: 2 }), cell("default", "Default", { width: 0.6, render: (r) => (c(r, "default") ? "Yes" : "—") }), ageCol],
    actions: ["edit", "delete"],
  },
  networkpolicies: {
    key: "networkpolicies",
    ref: K("networking.k8s.io", "v1", "NetworkPolicy", "networkpolicies", true),
    title: "Network policies",
    columns: [nameCol, nsCol, cell("podSelector", "Pod selector", { width: 2 }), cell("policyTypes", "Types"), cell("ingressRules", "Ingress", { width: 0.5, align: "right" }), cell("egressRules", "Egress", { width: 0.5, align: "right" }), ageCol],
    actions: ["edit", "delete"],
  },
  endpointslices: {
    key: "endpointslices",
    ref: K("discovery.k8s.io", "v1", "EndpointSlice", "endpointslices", true),
    title: "Endpoint slices",
    columns: [nameCol, nsCol, cell("addressType", "Type", { width: 0.6 }), cell("endpoints", "Endpoints", { width: 0.6, align: "right" }), cell("ownerName", "Service", { width: 1.4 }), ageCol],
    actions: ["edit", "delete"],
  },
  gatewayclasses: {
    key: "gatewayclasses",
    optional: true,
    ref: K("gateway.networking.k8s.io", "v1", "GatewayClass", "gatewayclasses", false),
    title: "Gateway classes",
    columns: [nameCol, cell("controller", "Controller", { width: 2 }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  gateways: {
    key: "gateways",
    optional: true,
    ref: K("gateway.networking.k8s.io", "v1", "Gateway", "gateways", true),
    title: "Gateways",
    columns: [nameCol, nsCol, cell("class", "Class"), cell("listeners", "Listeners", { width: 1.4 }), cell("addresses", "Addresses", { width: 1.2 }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  httproutes: {
    key: "httproutes",
    optional: true,
    ref: K("gateway.networking.k8s.io", "v1", "HTTPRoute", "httproutes", true),
    title: "HTTP routes",
    columns: [nameCol, nsCol, cell("hostnames", "Hostnames", { width: 2 }), cell("parents", "Gateways", { width: 1.2 }), cell("rules", "Rules", { width: 0.5, align: "right" }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  grpcroutes: {
    key: "grpcroutes",
    optional: true,
    ref: K("gateway.networking.k8s.io", "v1", "GRPCRoute", "grpcroutes", true),
    title: "gRPC routes",
    columns: [nameCol, nsCol, cell("hostnames", "Hostnames", { width: 2 }), cell("parents", "Gateways", { width: 1.2 }), statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  pvcs: {
    key: "pvcs",
    ref: K("", "v1", "PersistentVolumeClaim", "persistentvolumeclaims", true),
    title: "Volume claims",
    columns: [nameCol, nsCol, statusCol, cell("volume", "Volume", { width: 1.6 }), cell("capacity", "Capacity", { width: 0.7, align: "right" }), cell("accessModes", "Access"), cell("storageClass", "Class"), ageCol],
    actions: ["edit", "delete"],
  },
  pvs: {
    key: "pvs",
    ref: K("", "v1", "PersistentVolume", "persistentvolumes", false),
    title: "Persistent volumes",
    columns: [nameCol, cell("capacity", "Capacity", { width: 0.7, align: "right" }), cell("accessModes", "Access"), cell("reclaimPolicy", "Reclaim"), statusCol, cell("claim", "Claim", { width: 1.6 }), cell("storageClass", "Class"), ageCol],
    actions: ["edit", "delete"],
  },
  storageclasses: {
    key: "storageclasses",
    ref: K("storage.k8s.io", "v1", "StorageClass", "storageclasses", false),
    title: "Storage classes",
    columns: [
      { ...nameCol, render: (r) => <span className="font-medium">{r.name}{c(r, "default") && <span className="ml-2 text-[11px] text-accent">default</span>}</span> },
      cell("provisioner", "Provisioner", { width: 2 }),
      cell("reclaimPolicy", "Reclaim"),
      cell("volumeBindingMode", "Binding", { width: 1.2 }),
      cell("allowExpansion", "Expansion", { width: 0.6, render: (r) => (c(r, "allowExpansion") ? "Yes" : "No") }),
      ageCol,
    ],
    actions: ["edit", "delete"],
  },
  configmaps: {
    key: "configmaps",
    ref: K("", "v1", "ConfigMap", "configmaps", true),
    title: "Config maps",
    columns: [nameCol, nsCol, cell("keys", "Keys", { width: 0.5, align: "right" }), ageCol],
    actions: ["edit", "delete"],
  },
  secrets: {
    key: "secrets",
    ref: K("", "v1", "Secret", "secrets", true),
    title: "Secrets",
    columns: [nameCol, nsCol, cell("type", "Type", { width: 2 }), cell("keys", "Keys", { width: 0.5, align: "right" }), ageCol],
    actions: ["edit", "delete"],
  },
  serviceaccounts: {
    key: "serviceaccounts",
    ref: K("", "v1", "ServiceAccount", "serviceaccounts", true),
    title: "Service accounts",
    columns: [nameCol, nsCol, cell("secrets", "Secrets", { width: 0.5, align: "right" }), ageCol],
    actions: ["edit", "delete"],
  },
  roles: {
    key: "roles",
    ref: K("rbac.authorization.k8s.io", "v1", "Role", "roles", true),
    title: "Roles",
    columns: [nameCol, nsCol, cell("rules", "Rules", { width: 0.5, align: "right" }), ageCol],
    actions: ["edit", "delete"],
  },
  rolebindings: {
    key: "rolebindings",
    ref: K("rbac.authorization.k8s.io", "v1", "RoleBinding", "rolebindings", true),
    title: "Role bindings",
    columns: [nameCol, nsCol, cell("role", "Role", { width: 1.4 }), cell("subjects", "Subjects", { width: 2.4 }), ageCol],
    actions: ["edit", "delete"],
  },
  clusterroles: {
    key: "clusterroles",
    ref: K("rbac.authorization.k8s.io", "v1", "ClusterRole", "clusterroles", false),
    title: "Cluster roles",
    columns: [nameCol, cell("rules", "Rules", { width: 0.5, align: "right" }), cell("aggregated", "Aggregated", { width: 0.6, render: (r) => (c(r, "aggregated") ? "Yes" : "—") }), ageCol],
    actions: ["edit", "delete"],
  },
  clusterrolebindings: {
    key: "clusterrolebindings",
    ref: K("rbac.authorization.k8s.io", "v1", "ClusterRoleBinding", "clusterrolebindings", false),
    title: "Cluster role bindings",
    columns: [nameCol, cell("role", "Role", { width: 1.4 }), cell("subjects", "Subjects", { width: 2.4 }), ageCol],
    actions: ["edit", "delete"],
  },
  nodes: {
    key: "nodes",
    ref: K("", "v1", "Node", "nodes", false),
    title: "Nodes",
    columns: [
      nameCol,
      statusCol,
      cell("roles", "Roles"),
      cell("version", "Version", { width: 0.9 }),
      cell("internalIP", "Internal IP", { mono: true }),
      cell("cpuCapacityMillis", "CPU", { width: 0.7, align: "right", render: (r) => <span className="tabular">{cpu(c(r, "cpuCapacityMillis"))}</span> }),
      cell("memoryCapacityBytes", "Memory", { width: 0.8, align: "right", render: (r) => <span className="tabular">{bytes(c(r, "memoryCapacityBytes"))}</span> }),
      cell("taints", "Taints", { width: 0.5, align: "right" }),
      ageCol,
    ],
    actions: ["cordon", "drain", "edit", "delete"],
  },
  namespaces: {
    key: "namespaces",
    ref: K("", "v1", "Namespace", "namespaces", false),
    title: "Namespaces",
    columns: [nameCol, statusCol, ageCol],
    actions: ["edit", "delete"],
  },
  events: {
    key: "events",
    ref: K("", "v1", "Event", "events", true),
    title: "Events",
    columns: [
      cell("type", "Type", { width: 0.7, render: (r) => <StatusText health={r.health}>{c(r, "type")}</StatusText> }),
      cell("reason", "Reason", { width: 1.1 }),
      cell("object", "Object", {
        width: 1.8,
        render: (r) => <span className="truncate">{c(r, "objectKind")}/{c(r, "objectName")}</span>,
        sort: (r) => `${c(r, "objectKind")}/${c(r, "objectName")}`,
      }),
      cell("message", "Message", { width: 4, render: (r) => <span className="truncate text-muted">{c(r, "message")}</span> }),
      nsCol,
      cell("count", "Count", { width: 0.5, align: "right" }),
      cell("lastSeen", "Last seen", {
        width: 0.7,
        align: "right",
        render: (r) => <span className="tabular text-muted">{age(c(r, "lastSeen"))}</span>,
        sort: (r) => -(Date.parse(c(r, "lastSeen") ?? "") || 0),
      }),
    ],
    actions: ["edit", "delete"],
  },
  crds: {
    key: "crds",
    ref: K("apiextensions.k8s.io", "v1", "CustomResourceDefinition", "customresourcedefinitions", false),
    title: "Definitions",
    columns: [nameCol, cell("group", "Group", { width: 1.6 }), cell("kindName", "Kind"), cell("versions", "Versions", { width: 0.8 }), cell("scope", "Scope", { width: 0.8 }), ageCol],
    actions: ["edit", "delete"],
  },
};

/** Columns for arbitrary (custom) resources. */
export function genericKind(info: ApiResourceInfo): KindDef {
  const ref = K(info.group, info.version, info.kind, info.plural, info.namespaced);
  return {
    key: `${info.plural}.${info.group}`,
    ref,
    title: info.kind,
    columns: info.namespaced ? [nameCol, nsCol, statusCol, ageCol] : [nameCol, statusCol, ageCol],
    actions: ["edit", "delete"],
  };
}

export function kindByRef(ref: ResourceRef): KindDef {
  const found = Object.values(KINDS).find((k) => k.ref.kind === ref.kind && k.ref.group === ref.group);
  if (found) return { ...found, ref: { ...found.ref, version: ref.version } };
  return genericKind({ ...ref, verbs: [], subresources: [], custom: true });
}

/** Resolves the served version of a kind from discovery (falls back to the default). */
export function resolveKind(def: KindDef, apis?: ApiResourceInfo[]): KindDef | null {
  if (!apis) return def.optional ? null : def;
  const served = apis.find((a) => a.group === def.ref.group && a.kind === def.ref.kind);
  if (!served) return def.optional ? null : def;
  if (served.version === def.ref.version) return def;
  return { ...def, ref: { ...def.ref, version: served.version } };
}

export const VIEW_KINDS: Record<string, string[]> = {
  workloads: ["deployments", "statefulsets", "daemonsets", "replicasets", "jobs", "cronjobs", "hpas", "pdbs"],
  networking: ["services", "ingresses", "ingressclasses", "networkpolicies", "endpointslices", "gateways", "httproutes", "grpcroutes", "gatewayclasses"],
  storage: ["pvcs", "pvs", "storageclasses"],
  config: ["configmaps", "secrets", "namespaces"],
  access: ["serviceaccounts", "roles", "rolebindings", "clusterroles", "clusterrolebindings"],
};
