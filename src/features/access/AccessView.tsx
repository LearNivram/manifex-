import { useQuery } from "@tanstack/react-query";
import { Check, Fingerprint, Minus } from "lucide-react";
import { useState } from "react";
import { Badge, ErrorBanner, Section, Spinner, Tabs } from "@/components/ui";
import { api, errorMessage } from "@/lib/ipc";
import type { AccessCheck } from "@/lib/types";
import { VIEW_KINDS } from "@/features/resources/catalog";
import { ResourceView, ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useNamespace } from "@/store/app";

const MATRIX_RESOURCES: { label: string; group: string; resource: string; namespaced: boolean }[] = [
  { label: "Pods", group: "", resource: "pods", namespaced: true },
  { label: "Pod logs", group: "", resource: "pods/log", namespaced: true },
  { label: "Pod exec", group: "", resource: "pods/exec", namespaced: true },
  { label: "Deployments", group: "apps", resource: "deployments", namespaced: true },
  { label: "StatefulSets", group: "apps", resource: "statefulsets", namespaced: true },
  { label: "Services", group: "", resource: "services", namespaced: true },
  { label: "Ingresses", group: "networking.k8s.io", resource: "ingresses", namespaced: true },
  { label: "ConfigMaps", group: "", resource: "configmaps", namespaced: true },
  { label: "Secrets", group: "", resource: "secrets", namespaced: true },
  { label: "Jobs", group: "batch", resource: "jobs", namespaced: true },
  { label: "Namespaces", group: "", resource: "namespaces", namespaced: false },
  { label: "Nodes", group: "", resource: "nodes", namespaced: false },
  { label: "Roles", group: "rbac.authorization.k8s.io", resource: "roles", namespaced: true },
  { label: "CRDs", group: "apiextensions.k8s.io", resource: "customresourcedefinitions", namespaced: false },
];
const VERBS = ["get", "list", "watch", "create", "update", "patch", "delete"];

function PermissionMatrix({ clusterId, namespace }: { clusterId: string; namespace: string }) {
  const q = useQuery({
    queryKey: ["perm-matrix", clusterId, namespace],
    queryFn: async () => {
      const checks: AccessCheck[] = [];
      for (const r of MATRIX_RESOURCES) {
        const [resource, subresource] = r.resource.split("/");
        for (const verb of VERBS) checks.push({ verb: subresource === "exec" && verb === "create" ? "create" : verb, group: r.group, resource, subresource, namespace: r.namespaced ? namespace || undefined : undefined });
      }
      // chunk to respect the 100-check limit
      const out: boolean[] = [];
      for (let i = 0; i < checks.length; i += 90) out.push(...(await api.canI(clusterId, checks.slice(i, i + 90))));
      return out;
    },
  });
  const rules = useQuery({ queryKey: ["rbac-rules", clusterId, namespace], queryFn: () => api.rbacRules(clusterId, namespace || "default") });
  return (
    <div className="mx-auto flex max-w-[1200px] flex-col gap-4">
      <Section
        title={
          <span className="flex items-center gap-2">
            <Fingerprint className="size-4 text-muted" /> Effective permissions {rules.data?.identity ? `for ${rules.data.identity}` : ""}
          </span>
        }
        actions={<Badge>{namespace ? `namespace ${namespace}` : "all namespaces"}</Badge>}
      >
        {q.isLoading && <Spinner className="m-4" />}
        {q.error && <ErrorBanner className="m-4" message={errorMessage(q.error)} />}
        {q.data && (
          <div className="overflow-auto">
            <table className="w-full text-[12.5px]">
              <thead>
                <tr className="border-b border-border text-muted">
                  <th className="px-4 py-2 text-left font-medium">Resource</th>
                  {VERBS.map((v) => (
                    <th key={v} className="px-2 py-2 text-center font-medium">
                      {v}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {MATRIX_RESOURCES.map((r, ri) => (
                  <tr key={r.resource} className="border-b border-border/60 last:border-0">
                    <td className="px-4 py-1.5 font-medium">{r.label}</td>
                    {VERBS.map((v, vi) => {
                      const ok = q.data[ri * VERBS.length + vi];
                      return (
                        <td key={v} className="px-2 py-1.5 text-center">
                          {ok ? <Check className="mx-auto size-4 text-ok" /> : <Minus className="mx-auto size-4 text-subtle" />}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>
      {rules.data && (
        <Section title={`Rules in namespace ${rules.data.namespace}`} actions={rules.data.incomplete ? <Badge tone="warning">incomplete</Badge> : null}>
          <div className="max-h-96 divide-y divide-border overflow-auto font-mono text-[11.5px]">
            {rules.data.resourceRules.map((r, i) => (
              <div key={i} className="selectable grid grid-cols-[1fr_1.4fr_1fr] gap-3 px-4 py-1.5">
                <span className="truncate">{r.verbs.join(",")}</span>
                <span className="truncate">{r.resources.join(",")}</span>
                <span className="truncate text-muted">{r.apiGroups.map((g) => g || "core").join(",")}</span>
              </div>
            ))}
          </div>
        </Section>
      )}
    </div>
  );
}

export function AccessView() {
  const active = useActiveCluster()!;
  const ns = useNamespace();
  const [tab, setTab] = useState<"mine" | "objects">("mine");
  return (
    <div className="flex h-full flex-col">
      {tab === "mine" ? (
        <>
          <ViewHeader title="Access control" description="Kubernetes RBAC as seen by your identity – Manifex disables actions you are not allowed to perform.">
            <Tabs value={tab} onChange={setTab} tabs={[{ id: "mine", label: "My permissions" }, { id: "objects", label: "RBAC objects" }]} />
          </ViewHeader>
          <div className="min-h-0 flex-1 overflow-auto p-5">
            <PermissionMatrix clusterId={active.profile.id} namespace={ns} />
          </div>
        </>
      ) : (
        <>
          <div className="shrink-0 border-b border-border bg-panel px-4 py-1.5">
            <Tabs value={tab} onChange={setTab} tabs={[{ id: "mine", label: "My permissions" }, { id: "objects", label: "RBAC objects" }]} />
          </div>
          <div className="min-h-0 flex-1">
            <ResourceView clusterId={active.profile.id} title="RBAC objects" kindKeys={VIEW_KINDS.access} />
          </div>
        </>
      )}
    </div>
  );
}
