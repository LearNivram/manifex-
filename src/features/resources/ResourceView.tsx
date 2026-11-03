import { useQuery } from "@tanstack/react-query";
import { Lock, MoreHorizontal, Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  Button,
  EmptyState,
  ErrorBanner,
  Input,
  Menu,
  MenuContent,
  MenuItem,
  MenuSeparator,
  MenuTrigger,
  Spinner,
  Tabs,
} from "@/components/ui";
import { ResourceTable } from "@/components/table/ResourceTable";
import { useCanI } from "@/hooks/useCanI";
import { useWatch } from "@/hooks/useWatch";
import { matchesFilter } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { ApiResourceInfo, Row } from "@/lib/types";
import { useApp, useNamespace } from "@/store/app";
import { ACTION_LABELS, openBulkDelete, runAction } from "./actions";
import { KINDS, resolveKind, type ActionId, type KindDef } from "./catalog";

export function useApiResources(clusterId: string | null | undefined) {
  return useQuery({
    queryKey: ["api-resources", clusterId],
    enabled: !!clusterId,
    staleTime: 5 * 60_000,
    queryFn: () => api.listApiResources(clusterId!),
  });
}

export function ActionMenu({ clusterId, def, row }: { clusterId: string; def: KindDef; row: Row }) {
  const can = useCanI(clusterId, def.ref, def.actions, row.namespace);
  const primary = def.actions.filter((a) => a !== "delete");
  return (
    <Menu>
      <MenuTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label="Actions">
          <MoreHorizontal className="size-4" />
        </Button>
      </MenuTrigger>
      <MenuContent>
        {primary.map((a) => (
          <MenuItem
            key={a}
            disabled={!can(a)}
            icon={!can(a) ? <Lock className="size-3.5" /> : undefined}
            onSelect={() => runAction(a, { clusterId, ref: def.ref, namespace: row.namespace, name: row.name, row })}
          >
            {actionLabel(a, row)}
          </MenuItem>
        ))}
        {def.actions.includes("delete") && (
          <>
            <MenuSeparator />
            <MenuItem
              danger
              disabled={!can("delete")}
              icon={<Trash2 className="size-3.5" />}
              onSelect={() => runAction("delete", { clusterId, ref: def.ref, namespace: row.namespace, name: row.name, row })}
            >
              Delete…
            </MenuItem>
          </>
        )}
      </MenuContent>
    </Menu>
  );
}

export function actionLabel(a: ActionId, row?: Row): string {
  if (a === "cordon") return row?.cells.unschedulable ? "Uncordon" : "Cordon";
  if (a === "suspend") return row?.cells.suspend ? "Resume" : "Suspend";
  if (a === "pause") return row?.cells.paused ? "Resume rollout" : "Pause rollout";
  return ACTION_LABELS[a];
}

function KindTable({ clusterId, def, filter, namespace }: { clusterId: string; def: KindDef; filter: string; namespace: string }) {
  const select = useApp((s) => s.select);
  const selection = useApp((s) => s.selection);
  const { rows, ready, error, forbidden } = useWatch(clusterId, def.ref, namespace);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const filtered = useMemo(
    () => (filter ? rows.filter((r) => matchesFilter(`${r.name} ${r.namespace ?? ""} ${r.status} ${Object.values(r.labels).join(" ")}`, filter)) : rows),
    [rows, filter],
  );
  useEffect(() => setChecked(new Set()), [def.key, namespace]);

  if (forbidden) {
    return <EmptyState icon={<Lock className="size-5" />} title="Access denied" description={`Your identity is not allowed to list ${def.title.toLowerCase()}${namespace ? ` in ${namespace}` : ""}.`} />;
  }
  return (
    <div className="flex h-full min-h-0 flex-col">
      {error && <ErrorBanner className="m-3" message={error} />}
      {checked.size > 0 && (
        <div className="flex h-10 shrink-0 items-center gap-3 border-b border-border bg-accent-soft px-4 text-[12.5px]">
          <span className="font-medium">{checked.size} selected</span>
          <Button
            size="sm"
            variant="danger"
            icon={<Trash2 className="size-3.5" />}
            onClick={() =>
              openBulkDelete(
                rows.filter((r) => checked.has(r.uid)).map((r) => ({ clusterId, ref: def.ref, namespace: r.namespace, name: r.name, row: r })),
              )
            }
          >
            Delete
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setChecked(new Set())}>
            Clear
          </Button>
        </div>
      )}
      <div className="min-h-0 flex-1">
        {!ready ? (
          <div className="flex h-40 items-center justify-center gap-2 text-muted">
            <Spinner /> Loading {def.title.toLowerCase()}…
          </div>
        ) : (
          <ResourceTable
            rows={filtered}
            columns={def.ref.namespaced && namespace ? def.columns.filter((c) => c.id !== "namespace") : def.columns}
            selectedUid={selection?.name ? rows.find((r) => r.name === selection.name && (r.namespace ?? null) === (selection.namespace ?? null))?.uid : null}
            onSelect={(r) => select({ clusterId, resource: def.ref, namespace: r.namespace, name: r.name })}
            onActivate={(r) => select({ clusterId, resource: def.ref, namespace: r.namespace, name: r.name, tab: "yaml" })}
            rowActions={(r) => <ActionMenu clusterId={clusterId} def={def} row={r} />}
            initialSort={def.key === "events" ? { id: "lastSeen" } : undefined}
            checked={def.actions.includes("delete") && def.key !== "events" ? checked : undefined}
            onCheckedChange={setChecked}
            empty={
              <EmptyState
                title={filter ? "No matches" : `No ${def.title.toLowerCase()}`}
                description={filter ? "Try a different filter." : namespace ? `There are no ${def.title.toLowerCase()} in namespace ${namespace}.` : undefined}
              />
            }
          />
        )}
      </div>
    </div>
  );
}

export function useResolvedKinds(clusterId: string | null, keys: string[]): { kinds: KindDef[]; apis?: ApiResourceInfo[] } {
  const apis = useApiResources(clusterId).data;
  const kinds = useMemo(
    () => keys.map((k) => resolveKind(KINDS[k], apis)).filter((k): k is KindDef => !!k),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [apis, keys.join(",")],
  );
  return { kinds, apis };
}

export function ResourceView({ clusterId, title, kindKeys, description }: { clusterId: string; title: string; kindKeys: string[]; description?: string }) {
  const namespace = useNamespace();
  const viewParams = useApp((s) => s.viewParams);
  const openEditor = useApp((s) => s.openEditor);
  const { kinds } = useResolvedKinds(clusterId, kindKeys);
  const [active, setActive] = useState<string>(viewParams.kind && kindKeys.includes(viewParams.kind) ? viewParams.kind : kindKeys[0]);
  const [filter, setFilter] = useState("");
  useEffect(() => {
    if (viewParams.kind && kindKeys.includes(viewParams.kind)) setActive(viewParams.kind);
  }, [viewParams.kind, kindKeys]);
  const def = kinds.find((k) => k.key === active) ?? kinds[0];
  if (!def) return null;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <ViewHeader
        title={title}
        description={description}
        actions={
          <>
            <Input
              icon={<Search className="size-3.5" />}
              placeholder={`Filter ${def.title.toLowerCase()}…`}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              className="w-64"
            />
            <Button variant="primary" icon={<Plus className="size-4" />} onClick={() => openEditor({ mode: "create", clusterId, namespace, initial: template(def, namespace) })}>
              Create
            </Button>
          </>
        }
      >
        {kinds.length > 1 && <Tabs className="-mb-px" value={def.key} onChange={setActive} tabs={kinds.map((k) => ({ id: k.key, label: k.title }))} />}
      </ViewHeader>
      <div className="min-h-0 flex-1">
        <KindTable key={`${clusterId}-${def.key}`} clusterId={clusterId} def={def} filter={filter} namespace={def.ref.namespaced ? namespace : ""} />
      </div>
    </div>
  );
}

export function ViewHeader({ title, description, actions, children }: { title: React.ReactNode; description?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="shrink-0 border-b border-border bg-panel">
      <div className="flex min-h-14 items-center justify-between gap-4 px-5 py-2.5">
        <div className="flex min-w-0 flex-col">
          <h1 className="truncate text-[16px] font-semibold tracking-tight text-fg">{title}</h1>
          {description && <p className="truncate text-[12.5px] text-muted">{description}</p>}
        </div>
        <div className="flex shrink-0 items-center gap-2">{actions}</div>
      </div>
      {children && <div className="px-4 pb-2">{children}</div>}
    </div>
  );
}

export function template(def: KindDef, namespace: string): string {
  const ns = def.ref.namespaced ? `\n  namespace: ${namespace || "default"}` : "";
  const apiVersion = def.ref.group ? `${def.ref.group}/${def.ref.version}` : def.ref.version;
  switch (def.ref.kind) {
    case "Deployment":
      return `apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: my-app${ns}\n  labels:\n    app.kubernetes.io/name: my-app\nspec:\n  replicas: 2\n  selector:\n    matchLabels:\n      app.kubernetes.io/name: my-app\n  template:\n    metadata:\n      labels:\n        app.kubernetes.io/name: my-app\n    spec:\n      containers:\n        - name: app\n          image: nginx:1.27\n          ports:\n            - containerPort: 80\n          resources:\n            requests:\n              cpu: 50m\n              memory: 64Mi\n            limits:\n              memory: 128Mi\n`;
    case "Service":
      return `apiVersion: v1\nkind: Service\nmetadata:\n  name: my-app${ns}\nspec:\n  selector:\n    app.kubernetes.io/name: my-app\n  ports:\n    - port: 80\n      targetPort: 80\n`;
    case "ConfigMap":
      return `apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: my-config${ns}\ndata:\n  key: value\n`;
    case "Secret":
      return `apiVersion: v1\nkind: Secret\nmetadata:\n  name: my-secret${ns}\ntype: Opaque\nstringData:\n  password: change-me\n`;
    case "Namespace":
      return `apiVersion: v1\nkind: Namespace\nmetadata:\n  name: my-namespace\n`;
    case "CronJob":
      return `apiVersion: batch/v1\nkind: CronJob\nmetadata:\n  name: my-job${ns}\nspec:\n  schedule: "*/15 * * * *"\n  jobTemplate:\n    spec:\n      template:\n        spec:\n          restartPolicy: OnFailure\n          containers:\n            - name: job\n              image: busybox:1.36\n              command: ["sh", "-c", "date"]\n`;
    case "Ingress":
      return `apiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata:\n  name: my-app${ns}\nspec:\n  rules:\n    - host: my-app.example.com\n      http:\n        paths:\n          - path: /\n            pathType: Prefix\n            backend:\n              service:\n                name: my-app\n                port:\n                  number: 80\n`;
    case "PersistentVolumeClaim":
      return `apiVersion: v1\nkind: PersistentVolumeClaim\nmetadata:\n  name: my-data${ns}\nspec:\n  accessModes: ["ReadWriteOnce"]\n  resources:\n    requests:\n      storage: 1Gi\n`;
    default:
      return `apiVersion: ${apiVersion}\nkind: ${def.ref.kind}\nmetadata:\n  name: example${ns}\n`;
  }
}
