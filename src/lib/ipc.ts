// Typed bridge to the Rust core. Every call goes through Tauri's IPC; the webview
// holds no credentials and has no file-system access.

import { Channel, invoke } from "@tauri-apps/api/core";
import type * as T from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export class IpcError extends Error {
  code: string;
  status?: number;
  constructor(e: T.AppError) {
    super(e.message);
    this.code = e.code;
    this.status = e.status;
  }
}

export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as any).message);
  return "Unknown error";
}

async function call<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  if (!isTauri) throw new IpcError({ code: "unavailable", message: "Manifex must run inside the desktop application." });
  try {
    return await invoke<R>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "code" in (e as any)) throw new IpcError(e as T.AppError);
    throw new IpcError({ code: "internal", message: errorMessage(e) });
  }
}

export const api = {
  // clusters
  listClusters: () => call<T.ClusterView[]>("list_clusters"),
  discoverKubeconfigs: () => call<T.DiscoveredKubeconfig[]>("discover_kubeconfigs"),
  pickKubeconfigFile: () => call<T.DiscoveredKubeconfig | null>("pick_kubeconfig_file"),
  addKubeconfigClusters: (entries: { path: string; context: string; name?: string; storeInVault?: boolean; group?: string; environment?: string }[]) =>
    call<T.ClusterProfile[]>("add_kubeconfig_clusters", { entries }),
  testDirectCluster: (request: DirectClusterRequest) => call<T.ConnectionTestResult>("test_direct_cluster", { request }),
  addDirectCluster: (request: DirectClusterRequest) => call<T.ClusterProfile>("add_direct_cluster", { request }),
  sshDefaultKeys: () => call<T.SshKeyInfo[]>("ssh_default_keys"),
  pickSshKeyFile: () => call<T.SshKeyInfo | null>("pick_ssh_key_file"),
  sshProbeHost: (host: string, port: number) => call<T.HostKeyInfo>("ssh_probe_host", { host, port }),
  testSshCluster: (request: SshClusterRequest) => call<T.SshTestResult>("test_ssh_cluster", { request }),
  addSshCluster: (request: SshClusterRequest) => call<T.ClusterProfile>("add_ssh_cluster", { request }),
  updateCluster: (id: string, patch: Partial<Record<string, unknown>>) => call<T.ClusterProfile>("update_cluster", { id, patch }),
  removeCluster: (id: string) => call<void>("remove_cluster", { id }),
  testCluster: (id: string) => call<T.ConnectionTestResult>("test_cluster", { id }),
  connectCluster: (id: string) => call<T.ClusterStatus>("connect_cluster", { id }),
  disconnectCluster: (id: string) => call<void>("disconnect_cluster", { id }),
  listApiResources: (clusterId: string, refresh = false) => call<T.ApiResourceInfo[]>("list_api_resources", { clusterId, refresh }),

  // resources
  watchResources: (
    clusterId: string,
    resource: T.ResourceRef,
    namespace: string | null,
    onEvent: Channel<T.WatchMessage>,
    labelSelector?: string | null,
    fieldSelector?: string | null,
  ) => call<string>("watch_resources", { clusterId, resource, namespace, labelSelector, fieldSelector, onEvent }),
  unwatchResources: (subscriptionId: string) => call<void>("unwatch_resources", { subscriptionId }),
  listResources: (clusterId: string, resource: T.ResourceRef, namespace?: string | null, labelSelector?: string, fieldSelector?: string, limit?: number) =>
    call<T.Row[]>("list_resources", { clusterId, resource, namespace, labelSelector, fieldSelector, limit }),
  getResource: (clusterId: string, resource: T.ResourceRef, namespace: string | null | undefined, name: string) =>
    call<T.ObjectView>("get_resource", { clusterId, resource, namespace, name }),
  applyYaml: (clusterId: string, yaml: string, opts: { namespace?: string | null; mode: "apply" | "replace"; dryRun: boolean; forceConflicts?: boolean }) =>
    call<T.ApplyResult[]>("apply_yaml", { clusterId, yaml, ...opts }),
  deleteResource: (clusterId: string, resource: T.ResourceRef, namespace: string | null | undefined, name: string, options?: { propagation?: string; gracePeriodSeconds?: number }, confirm?: string) =>
    call<void>("delete_resource", { clusterId, resource, namespace, name, options, confirm }),
  scaleResource: (clusterId: string, resource: T.ResourceRef, namespace: string, name: string, replicas: number) =>
    call<void>("scale_resource", { clusterId, resource, namespace, name, replicas }),
  restartWorkload: (clusterId: string, resource: T.ResourceRef, namespace: string, name: string) =>
    call<void>("restart_workload", { clusterId, resource, namespace, name }),
  setDeploymentPaused: (clusterId: string, namespace: string, name: string, paused: boolean) =>
    call<void>("set_deployment_paused", { clusterId, namespace, name, paused }),
  rolloutHistory: (clusterId: string, namespace: string, name: string) => call<T.RolloutRevision[]>("rollout_history", { clusterId, namespace, name }),
  rollbackDeployment: (clusterId: string, namespace: string, name: string, revision: number) =>
    call<void>("rollback_deployment", { clusterId, namespace, name, revision }),
  cordonNode: (clusterId: string, name: string, unschedulable: boolean) => call<void>("cordon_node", { clusterId, name, unschedulable }),
  drainNode: (clusterId: string, name: string, options: { ignoreDaemonsets: boolean; deleteEmptydirData: boolean; force: boolean }, confirm?: string) =>
    call<T.DrainReport>("drain_node", { clusterId, name, options, confirm }),
  suspendCronjob: (clusterId: string, namespace: string, name: string, suspend: boolean) =>
    call<void>("suspend_cronjob", { clusterId, namespace, name, suspend }),
  triggerCronjob: (clusterId: string, namespace: string, name: string) => call<string>("trigger_cronjob", { clusterId, namespace, name }),
  canI: (clusterId: string, checks: T.AccessCheck[]) => call<boolean[]>("can_i", { clusterId, checks }),
  rbacRules: (clusterId: string, namespace: string) => call<T.RulesReview>("rbac_rules", { clusterId, namespace }),
  resourceSchema: (clusterId: string, apiVersion: string, kind: string) => call<unknown>("resource_schema", { clusterId, apiVersion, kind }),
  nodeMetrics: (clusterId: string) => call<T.NodeMetric[]>("node_metrics", { clusterId }),
  podMetrics: (clusterId: string, namespace?: string | null) => call<T.PodMetric[]>("pod_metrics", { clusterId, namespace }),
  clusterCapacity: (clusterId: string) => call<T.ClusterCapacity>("cluster_capacity", { clusterId }),
  listApplications: (clusterId: string, namespace?: string | null) => call<T.Application[]>("list_applications", { clusterId, namespace }),
  globalSearch: (query: string, limit = 60) => call<T.SearchHit[]>("global_search", { query, limit }),
  compareClusters: (left: string, right: string, namespace?: string | null) => call<T.ClusterComparison>("compare_clusters", { left, right, namespace }),
  workloadPods: (clusterId: string, namespace: string, selector: string) => call<T.Row[]>("workload_pods", { clusterId, namespace, selector }),

  // streams
  logsStart: (
    clusterId: string,
    namespace: string,
    pod: string,
    options: { container?: string; follow: boolean; tailLines?: number; sinceSeconds?: number; timestamps: boolean; previous: boolean },
    onEvent: Channel<T.LogMessage>,
  ) => call<string>("logs_start", { clusterId, namespace, pod, options, onEvent }),
  logsStop: (streamId: string) => call<void>("logs_stop", { streamId }),
  execStart: (
    clusterId: string,
    namespace: string,
    pod: string,
    container: string | undefined,
    shell: string,
    cols: number,
    rows: number,
    onData: Channel<ArrayBuffer | number[]>,
    onEvent: Channel<T.ExecEvent>,
  ) => call<string>("exec_start", { clusterId, namespace, pod, container, shell, cols, rows, onData, onEvent }),
  execWrite: (sessionId: string, data: string) => call<void>("exec_write", { sessionId, data }),
  execResize: (sessionId: string, cols: number, rows: number) => call<void>("exec_resize", { sessionId, cols, rows }),
  execClose: (sessionId: string) => call<void>("exec_close", { sessionId }),

  // helm
  helmStatus: () => call<T.HelmStatus>("helm_status"),
  helmReleases: (clusterId: string, namespace?: string | null) => call<T.HelmRelease[]>("helm_releases", { clusterId, namespace }),
  helmRelease: (clusterId: string, namespace: string, name: string) => call<T.HelmReleaseDetail>("helm_release", { clusterId, namespace, name }),
  helmRollback: (clusterId: string, namespace: string, name: string, revision: number) =>
    call<T.HelmOutput>("helm_rollback", { clusterId, namespace, name, revision }),
  helmUninstall: (clusterId: string, namespace: string, name: string, keepHistory: boolean, confirm?: string) =>
    call<T.HelmOutput>("helm_uninstall", { clusterId, namespace, name, keepHistory, confirm }),
  helmUpgrade: (
    clusterId: string,
    request: { namespace: string; name: string; chart: string; version?: string; values?: string; createNamespace?: boolean; dryRun?: boolean },
  ) => call<T.HelmOutput>("helm_upgrade", { clusterId, request }),
  helmRepos: () => call<{ name: string; url: string }[]>("helm_repos"),
  helmRepoAdd: (name: string, url: string) => call<T.HelmOutput>("helm_repo_add", { name, url }),
  helmRepoRemove: (name: string) => call<T.HelmOutput>("helm_repo_remove", { name }),
  helmRepoUpdate: () => call<T.HelmOutput>("helm_repo_update"),
  helmSearch: (query: string) => call<{ name: string; version: string; app_version: string; description: string }[]>("helm_search", { query }),
  helmShowValues: (chart: string, version?: string) => call<string>("helm_show_values", { chart, version }),

  // system
  getSettings: () => call<T.AppSettings>("get_settings"),
  saveSettings: (settings: T.AppSettings) => call<T.AppSettings>("save_settings", { settings }),
  appInfo: () => call<T.AppInfo>("app_info"),
  listAlerts: () => call<T.Alert[]>("list_alerts"),
  clearAlerts: () => call<void>("clear_alerts"),
  listAlertRules: () => call<T.AlertRule[]>("list_alert_rules"),
  saveAlertRule: (rule: T.AlertRule) => call<T.AlertRule>("save_alert_rule", { rule }),
  deleteAlertRule: (id: string) => call<void>("delete_alert_rule", { id }),
  queryAudit: (query: T.AuditQuery) => call<T.AuditEntry[]>("query_audit", { query }),
  clearAudit: () => call<void>("clear_audit"),
  exportAudit: (query: T.AuditQuery, format: "csv" | "json") => call<string | null>("export_audit", { query, format }),
  saveTextFile: (defaultName: string, content: string) => call<string | null>("save_text_file", { defaultName, content }),
  openWindow: (clusterId?: string, view?: string) => call<string>("open_window", { clusterId, view }),
  takeDeepLinks: () => call<string[]>("take_deep_links"),
};

export interface DirectClusterRequest {
  name: string;
  kind: "token" | "certificate" | "agent";
  server: string;
  caPem?: string;
  tlsServerName?: string;
  proxyUrl?: string;
  token?: string;
  certPem?: string;
  keyPem?: string;
  group?: string;
  environment?: string;
}

export interface SshClusterRequest {
  name: string;
  host: string;
  port: number;
  user: string;
  keyPath?: string;
  keyPem?: string;
  certificate?: string;
  passphrase?: string;
  password?: string;
  hostKeyFingerprint: string;
  context?: string;
  group?: string;
  environment?: string;
}

export { Channel };
