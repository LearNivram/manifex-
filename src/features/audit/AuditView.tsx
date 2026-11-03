import { useQuery } from "@tanstack/react-query";
import { Download, FileClock, Search, Trash2 } from "lucide-react";
import { useState } from "react";
import { Badge, Button, EmptyState, Input, Select, Spinner } from "@/components/ui";
import { confirm } from "@/components/ui/confirm";
import { dateTime } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useApp } from "@/store/app";

export function AuditView() {
  const clusters = useApp((s) => s.clusters);
  const toast = useApp((s) => s.toast);
  const [clusterId, setClusterId] = useState("");
  const [outcome, setOutcome] = useState("");
  const [search, setSearch] = useState("");
  const query = { clusterId: clusterId || null, outcome: outcome || null, search: search || null, limit: 1000 };
  const q = useQuery({ queryKey: ["audit", query], queryFn: () => api.queryAudit(query), refetchInterval: 10_000 });

  const exportAs = async (format: "csv" | "json") => {
    try {
      const path = await api.exportAudit(query, format);
      if (path) toast({ kind: "success", title: "Audit log exported", message: path });
    } catch (e) {
      toast({ kind: "error", title: "Export failed", message: errorMessage(e) });
    }
  };

  return (
    <div className="flex h-full flex-col">
      <ViewHeader
        title="Audit log"
        description="Every change made through Manifex – applies, deletes, scaling, exec sessions, Helm operations – recorded locally with identity and outcome."
        actions={
          <>
            <Button icon={<Download className="size-4" />} onClick={() => exportAs("csv")}>
              CSV
            </Button>
            <Button icon={<Download className="size-4" />} onClick={() => exportAs("json")}>
              JSON
            </Button>
            <Button
              variant="ghost"
              icon={<Trash2 className="size-4" />}
              onClick={async () => {
                if (await confirm({ title: "Clear the local audit log?", message: "This removes all recorded entries from this machine.", danger: true, confirmLabel: "Clear" })) {
                  await api.clearAudit();
                  q.refetch();
                }
              }}
            >
              Clear
            </Button>
          </>
        }
      >
        <div className="flex gap-2 pt-1">
          <Input icon={<Search className="size-3.5" />} placeholder="Search action, name, namespace…" value={search} onChange={(e) => setSearch(e.target.value)} className="w-72" />
          <Select value={clusterId} onChange={setClusterId} options={[{ value: "", label: "All clusters" }, ...clusters.map((c) => ({ value: c.profile.id, label: c.profile.name }))]} className="w-48" />
          <Select
            value={outcome}
            onChange={setOutcome}
            options={[
              { value: "", label: "Any outcome" },
              { value: "success", label: "Success" },
              { value: "failure", label: "Failure" },
            ]}
            className="w-36"
          />
        </div>
      </ViewHeader>
      <div className="min-h-0 flex-1 overflow-auto">
        {q.isLoading && <Spinner className="m-4" />}
        {q.data?.length === 0 && <EmptyState icon={<FileClock className="size-5" />} title="No audit entries" />}
        {q.data && q.data.length > 0 && (
          <table className="w-full text-[12.5px]">
            <thead className="sticky top-0 bg-panel">
              <tr className="border-b border-border text-left text-[11.5px] text-muted">
                <th className="px-4 py-2 font-medium">Time</th>
                <th className="px-2 py-2 font-medium">Cluster</th>
                <th className="px-2 py-2 font-medium">Identity</th>
                <th className="px-2 py-2 font-medium">Action</th>
                <th className="px-2 py-2 font-medium">Object</th>
                <th className="px-2 py-2 font-medium">Outcome</th>
                <th className="px-4 py-2 font-medium">Detail</th>
              </tr>
            </thead>
            <tbody className="selectable">
              {q.data.map((e) => (
                <tr key={e.id} className="border-b border-border/60 align-top hover:bg-hover">
                  <td className="tabular px-4 py-1.5 whitespace-nowrap text-muted">{dateTime(e.ts)}</td>
                  <td className="px-2 py-1.5">{e.clusterName ?? "—"}</td>
                  <td className="px-2 py-1.5 text-muted">{e.identity ?? "—"}</td>
                  <td className="px-2 py-1.5 font-mono text-[12px]">{e.action}</td>
                  <td className="px-2 py-1.5">
                    {e.resource ? `${e.resource} ` : ""}
                    {e.namespace ? `${e.namespace}/` : ""}
                    {e.name ?? ""}
                  </td>
                  <td className="px-2 py-1.5">
                    <Badge tone={e.outcome === "success" ? "ok" : "error"}>{e.outcome}</Badge>
                  </td>
                  <td className="max-w-md px-4 py-1.5 break-words text-muted">{e.detail}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
