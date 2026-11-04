// Types mirroring the Rust core's IPC payloads (serde camelCase).

export type Health = "ok" | "warning" | "error" | "pending" | "neutral";

export interface ResourceRef {
  group: string;
  version: string;
  kind: string;
  plural: string;
  namespaced: boolean;
}

export interface Row {
  uid: string;
  name: string;
  namespace?: string | null;
  created?: string | null;
  resourceVersion?: string | null;
  labels: Record<string, string>;
  health: Health;
  status: string;
  cells: Record<string, any>;
}

export type WatchMessage =
  | { type: "snapshot"; rows: Row[]; ready: boolean }
  | { type: "batch"; upserts: Row[]; deletes: string[] }
  | { type: "error"; message: string; status?: number | null }
  | { type: "closed" };

export type ClusterSource =
  | { type: "kubeconfig"; path: string; context: string }
  | { type: "storedKubeconfig"; context: string; secretId: string }
  | { type: "token"; server: string; caPem?: string | null; tlsServerName?: string | null; proxyUrl?: string | null; secretId: string }
  | { type: "certificate"; server: string; caPem?: string | null; tlsServerName?: string | null; proxyUrl?: string | null; secretId: string }
  | { type: "agent"; endpoint: string; caPem?: string | null; proxyUrl?: string | null; secretId: string }
  | {
      type: "ssh";
      host: string;
      port: number;
      user: string;
      hostKeyFingerprint: string;
      context?: string | null;
      kubeconfigSource?: string | null;
      secretId: string;
    };

export type Environment = "development" | "staging" | "production" | "other";

export interface ClusterProfile {
  id: string;
  name: string;
  source: ClusterSource;
  group?: string | null;
  favorite: boolean;
  color?: string | null;
  environment: Environment;
  defaultNamespace?: string | null;
  alertsEnabled: boolean;
  sortOrder: number;
  createdAt: string;
  lastConnectedAt?: string | null;
}

export type ConnState = "disconnected" | "connecting" | "connected" | "reconnecting" | "error";

export interface ClusterStatus {
  id: string;
  name: string;
  state: ConnState;
  message?: string | null;
  server?: string | null;
  version?: string | null;
  platform?: string | null;
  identity?: string | null;
  latencyMs?: number | null;
  since: string;
}

export interface ClusterView {
  profile: ClusterProfile;
  status?: ClusterStatus | null;
}

export interface ContextInfo {
  name: string;
  cluster: string;
  user?: string | null;
  namespace?: string | null;
  server?: string | null;
  auth: string;
  insecure: boolean;
  alreadyAdded: boolean;
}

export interface DiscoveredKubeconfig {
  path: string;
  contexts: ContextInfo[];
  currentContext?: string | null;
  error?: string | null;
}

export interface ConnectionTestResult {
  ok: boolean;
  version?: string | null;
  platform?: string | null;
  identity?: string | null;
  latencyMs: number;
  message?: string | null;
}

export interface ApiResourceInfo {
  group: string;
  version: string;
  kind: string;
  plural: string;
  namespaced: boolean;
  verbs: string[];
  subresources: string[];
  custom: boolean;
}

export interface ObjectView {
  object: any;
  yaml: string;
}

export interface ApplyResult {
  apiVersion: string;
  kind: string;
  name: string;
  namespace?: string | null;
  action: "created" | "configured" | "replaced" | "error";
  dryRun: boolean;
  yaml?: string | null;
  previousYaml?: string | null;
  error?: string | null;
}

export interface RolloutRevision {
  revision: number;
  replicaSet: string;
  images: string[];
  created?: string | null;
  replicas: number;
  changeCause?: string | null;
  current: boolean;
}

export interface DrainReport {
  evicted: string[];
  skipped: [string, string][];
  failed: [string, string][];
}

export type LogMessage = { type: "lines"; lines: string[] } | { type: "end" } | { type: "error"; message: string };

export type ExecEvent = { type: "exit"; code?: number | null; message?: string | null } | { type: "error"; message: string };

export interface NodeMetric {
  name: string;
  cpuMillis: number;
  memoryBytes: number;
  timestamp?: string | null;
}

export interface PodMetric {
  name: string;
  namespace: string;
  cpuMillis: number;
  memoryBytes: number;
  containers: { name: string; cpuMillis: number; memoryBytes: number }[];
}

export interface ResourceTotals {
  capacity: number;
  allocatable: number;
  requests: number;
  limits: number;
  usage?: number | null;
}

export interface NodeCapacity {
  name: string;
  ready: boolean;
  cpu: ResourceTotals;
  memory: ResourceTotals;
  pods: number;
  podCapacity: number;
}

export interface ClusterCapacity {
  nodes: NodeCapacity[];
  cpu: ResourceTotals;
  memory: ResourceTotals;
  pods: number;
  podCapacity: number;
  metricsAvailable: boolean;
}

export interface SearchHit {
  clusterId: string;
  clusterName: string;
  resource: ResourceRef;
  namespace?: string | null;
  name: string;
}

export interface WorkloadFacts {
  replicas: number;
  ready: string;
  images: string[];
}

export interface ClusterComparison {
  left: ClusterFacts;
  right: ClusterFacts;
  onlyLeftNamespaces: string[];
  onlyRightNamespaces: string[];
  workloads: { namespace: string; name: string; kind: string; left?: WorkloadFacts | null; right?: WorkloadFacts | null; differs: boolean }[];
}

export interface ClusterFacts {
  clusterId: string;
  name: string;
  version?: string | null;
  nodes: number;
  readyNodes: number;
  cpuMillis: number;
  memoryBytes: number;
  namespaces: string[];
  pods: number;
}

export interface Application {
  name: string;
  namespace: string;
  source: string;
  version?: string | null;
  managedBy?: string | null;
  partOf?: string | null;
  health: Health;
  components: { kind: string; name: string; namespace: string; status: string; health: Health }[];
}

export interface HelmRelease {
  name: string;
  namespace: string;
  revision: number;
  status: string;
  chart: string;
  chartVersion: string;
  appVersion?: string | null;
  updated?: string | null;
  description?: string | null;
  icon?: string | null;
}

export interface HelmReleaseDetail {
  release: HelmRelease;
  history: { revision: number; status: string; chart: string; chartVersion: string; appVersion?: string | null; updated?: string | null; description?: string | null }[];
  values: string;
  chartValues: string;
  manifest: string;
  notes?: string | null;
  resources: { apiVersion: string; kind: string; name: string; namespace?: string | null }[];
}

export interface HelmStatus {
  available: boolean;
  version?: string | null;
  path?: string | null;
  message?: string | null;
}

export interface HelmOutput {
  stdout: string;
  stderr: string;
}

export interface AppSettings {
  theme: "system" | "dark" | "light";
  density: "comfortable" | "compact";
  strictProductionConfirmations: boolean;
  confirmDestructive: boolean;
  closeToTray: boolean;
  nativeNotifications: boolean;
  notifyConnectionLoss: boolean;
  notifyPodFailures: boolean;
  notifyDeploymentFailures: boolean;
  notifyNodeProblems: boolean;
  notifyWarningEvents: boolean;
  helmPath: string;
  logTailLines: number;
  logTimestamps: boolean;
  terminalFontSize: number;
  editorFontSize: number;
  globalShortcut: string;
  autoCheckUpdates: boolean;
  autoReconnect: boolean;
  healthIntervalSecs: number;
}

export interface Alert {
  id: string;
  ts: string;
  clusterId: string;
  clusterName: string;
  severity: "info" | "warning" | "critical";
  category: string;
  title: string;
  message: string;
  namespace?: string | null;
  objectKind?: string | null;
  objectName?: string | null;
  ruleId?: string | null;
}

export interface AlertRule {
  id: string;
  name: string;
  enabled: boolean;
  clusterId?: string | null;
  namespace?: string | null;
  kind?: string | null;
  reasonPattern?: string | null;
  messagePattern?: string | null;
  severity: "info" | "warning" | "critical";
  createdAt: string;
}

export interface AuditEntry {
  id: number;
  ts: string;
  clusterId?: string | null;
  clusterName?: string | null;
  identity?: string | null;
  action: string;
  resource?: string | null;
  namespace?: string | null;
  name?: string | null;
  outcome: string;
  detail?: string | null;
}

export interface AuditQuery {
  clusterId?: string | null;
  search?: string | null;
  outcome?: string | null;
  limit?: number;
  beforeId?: number | null;
}

export interface RulesReview {
  namespace: string;
  identity?: string | null;
  incomplete: boolean;
  evaluationError?: string | null;
  resourceRules: { verbs: string[]; apiGroups: string[]; resources: string[]; resourceNames: string[] }[];
  nonResourceRules: { verbs: string[]; resources: string[] }[];
}

export interface AppInfo {
  version: string;
  os: string;
  arch: string;
  dataDir: string;
  vaultAvailable: boolean;
  vaultMessage?: string | null;
  activeWatches: number;
  activeSubscriptions: number;
  terminalSessions: number;
}

export interface AppError {
  code: string;
  message: string;
  status?: number;
  reason?: string;
}

export interface AccessCheck {
  verb: string;
  group: string;
  resource: string;
  subresource?: string;
  namespace?: string;
  name?: string;
}

export interface SshKeyInfo {
  path: string;
  encrypted: boolean;
  algorithm: string;
  hasCertificate: boolean;
}

export interface HostKeyInfo {
  fingerprint: string;
  algorithm: string;
  knownHosts: "trusted" | "unknown" | "changed";
}

export interface SshTestResult {
  ok: boolean;
  message?: string | null;
  kubeconfigSource?: string | null;
  contexts: string[];
  context?: string | null;
  version?: string | null;
  identity?: string | null;
  latencyMs: number;
}
