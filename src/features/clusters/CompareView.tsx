import { useQuery } from "@tanstack/react-query";
import { GitCompare } from "lucide-react";
import { useState } from "react";
import { Badge, EmptyState, ErrorBanner, Field, Input, KeyValue, Section, Select, Spinner, Toggle } from "@/components/ui";
import { bytes, cpu } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { ClusterFacts } from "@/lib/types";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useApp } from "@/store/app";

function Facts({ f }: { f: ClusterFacts }) {
  return (
    <Section title={f.name}>
      <div className="p-4">
        <KeyValue
          items={[
            ["Version", f.version],
            ["Nodes", `${f.readyNodes}/${f.nodes} ready`],
            ["CPU", cpu(f.cpuMillis)],
            ["Memory", bytes(f.memoryBytes)],
            ["Pods", f.pods],
            ["Namespaces", f.namespaces.length],
          ]}
        />
      </div>
    </Section>
  );
}

export function CompareView() {
  const clusters = useApp((s) => s.clusters);
  const params = useApp((s) => s.viewParams);
  const connected = clusters.filter((c) => c.status?.state === "connected");
  const [left, setLeft] = useState(params.left ?? connected[0]?.profile.id ?? "");
  const [right, setRight] = useState(params.right ?? connected[1]?.profile.id ?? "");
  const [namespace, setNamespace] = useState("");
  const [onlyDiff, setOnlyDiff] = useState(true);
  const ready = left && right && left !== right;
  const q = useQuery({ queryKey: ["compare", left, right, namespace], enabled: !!ready, queryFn: () => api.compareClusters(left, right, namespace || null) });
  const options = [{ value: "", label: "Select cluster…" }, ...connected.map((c) => ({ value: c.profile.id, label: c.profile.name }))];
  const workloads = q.data?.workloads.filter((w) => !onlyDiff || w.differs) ?? [];

  return (
    <div className="flex h-full flex-col">
      <ViewHeader title="Compare clusters" description="Spot drift between environments: versions, capacity, namespaces and workload images / replicas.">
        <div className="flex flex-wrap items-end gap-3 pt-1">
          <Field label="Left">
            <Select value={left} onChange={setLeft} options={options} className="w-56" />
          </Field>
          <Field label="Right">
            <Select value={right} onChange={setRight} options={options} className="w-56" />
          </Field>
          <Field label="Namespace (optional)">
            <Input value={namespace} onChange={(e) => setNamespace(e.target.value)} placeholder="all" className="w-48" />
          </Field>
          <div className="pb-1.5">
            <Toggle checked={onlyDiff} onChange={setOnlyDiff} label="Only differences" />
          </div>
        </div>
      </ViewHeader>
      <div className="min-h-0 flex-1 overflow-auto p-5">
        {connected.length < 2 && <EmptyState icon={<GitCompare className="size-5" />} title="Connect at least two clusters" description="Comparison runs live against both clusters." />}
        {q.isFetching && <Spinner />}
        {q.error && <ErrorBanner message={errorMessage(q.error)} />}
        {q.data && (
          <div className="mx-auto flex max-w-[1400px] flex-col gap-4">
            <div className="grid grid-cols-2 gap-4">
              <Facts f={q.data.left} />
              <Facts f={q.data.right} />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <Section title={`Only in ${q.data.left.name}`}>
                <div className="flex flex-wrap gap-1.5 p-4">{q.data.onlyLeftNamespaces.length ? q.data.onlyLeftNamespaces.map((n) => <Badge key={n}>{n}</Badge>) : <span className="text-muted">—</span>}</div>
              </Section>
              <Section title={`Only in ${q.data.right.name}`}>
                <div className="flex flex-wrap gap-1.5 p-4">{q.data.onlyRightNamespaces.length ? q.data.onlyRightNamespaces.map((n) => <Badge key={n}>{n}</Badge>) : <span className="text-muted">—</span>}</div>
              </Section>
            </div>
            <Section title="Workloads" actions={<Badge>{workloads.length}</Badge>}>
              <div className="grid grid-cols-[1.6fr_0.8fr_2fr_2fr] gap-3 border-b border-border px-4 py-2 text-[11.5px] font-medium text-muted">
                <span>Workload</span>
                <span>Kind</span>
                <span>{q.data.left.name}</span>
                <span>{q.data.right.name}</span>
              </div>
              <div className="divide-y divide-border">
                {workloads.map((w) => (
                  <div key={`${w.kind}/${w.namespace}/${w.name}`} className="grid grid-cols-[1.6fr_0.8fr_2fr_2fr] gap-3 px-4 py-2 text-[12.5px]">
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate font-medium">{w.name}</span>
                      <span className="text-[11.5px] text-muted">{w.namespace}</span>
                    </span>
                    <span className="text-muted">{w.kind}</span>
                    {[w.left, w.right].map((side, i) => (
                      <span key={i} className="flex min-w-0 flex-col">
                        {side ? (
                          <>
                            <span className="truncate font-mono text-[11.5px]">{side.images.join(", ")}</span>
                            <span className="text-[11.5px] text-muted">
                              {side.replicas} replicas · {side.ready} ready
                            </span>
                          </>
                        ) : (
                          <Badge tone="warning">missing</Badge>
                        )}
                      </span>
                    ))}
                  </div>
                ))}
              </div>
            </Section>
          </div>
        )}
      </div>
    </div>
  );
}
