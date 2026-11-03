// Resource actions and their dialogs (delete, scale, rollout history, drain …).
// Every destructive action names the target cluster explicitly; production
// clusters require typing the resource name.

import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, RotateCcw } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { Badge, Button, Dialog, ErrorBanner, Field, Input, Select, Spinner, Toggle } from "@/components/ui";
import { age } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { DrainReport, ResourceRef, RolloutRevision, Row } from "@/lib/types";
import { useApp } from "@/store/app";
import type { ActionId } from "./catalog";

interface Target {
  clusterId: string;
  ref: ResourceRef;
  namespace?: string | null;
  name: string;
  row?: Row;
}

type DialogState =
  | { type: "delete"; targets: Target[] }
  | { type: "scale"; target: Target; current: number }
  | { type: "history"; target: Target }
  | { type: "drain"; target: Target }
  | null;

const useDialogs = create<{ dialog: DialogState; open: (d: DialogState) => void }>((set) => ({
  dialog: null,
  open: (dialog) => set({ dialog }),
}));

function clusterOf(id: string) {
  return useApp.getState().clusters.find((c) => c.profile.id === id)?.profile;
}

export function EnvBadge({ environment }: { environment: string }) {
  const tone = environment === "production" ? "error" : environment === "staging" ? "warning" : environment === "development" ? "ok" : "neutral";
  return <Badge tone={tone}>{environment}</Badge>;
}

function TargetCluster({ clusterId }: { clusterId: string }) {
  const p = clusterOf(clusterId);
  return (
    <div className="flex items-center gap-2 rounded-md border border-border bg-canvas px-3 py-2 text-[12.5px]">
      <span className="text-muted">Cluster</span>
      <span className="font-semibold text-fg">{p?.name ?? clusterId}</span>
      {p && <EnvBadge environment={p.environment} />}
    </div>
  );
}

/** Runs an action; dialogs are opened for actions that need input or confirmation. */
export async function runAction(action: ActionId, target: Target): Promise<void> {
  const app = useApp.getState();
  const { clusterId, ref, namespace, name, row } = target;
  const ok = (title: string) => app.toast({ kind: "success", title });
  const fail = (e: unknown) => app.toast({ kind: "error", title: "Operation failed", message: errorMessage(e) });
  switch (action) {
    case "delete":
      useDialogs.getState().open({ type: "delete", targets: [target] });
      return;
    case "scale":
      useDialogs.getState().open({ type: "scale", target, current: Number(row?.cells.replicas ?? row?.cells.desired ?? 1) });
      return;
    case "history":
      useDialogs.getState().open({ type: "history", target });
      return;
    case "drain":
      useDialogs.getState().open({ type: "drain", target });
      return;
    case "edit":
      app.openEditor({ mode: "edit", clusterId, resource: ref, namespace, name });
      return;
    case "restart":
      try {
        await api.restartWorkload(clusterId, ref, namespace!, name);
        ok(`Restarting ${ref.kind.toLowerCase()} ${name}`);
      } catch (e) {
        fail(e);
      }
      return;
    case "pause": {
      const paused = !row?.cells.paused;
      try {
        await api.setDeploymentPaused(clusterId, namespace!, name, paused);
        ok(paused ? `Rollout of ${name} paused` : `Rollout of ${name} resumed`);
      } catch (e) {
        fail(e);
      }
      return;
    }
    case "cordon": {
      const unschedulable = !row?.cells.unschedulable;
      try {
        await api.cordonNode(clusterId, name, unschedulable);
        ok(unschedulable ? `Node ${name} cordoned` : `Node ${name} uncordoned`);
      } catch (e) {
        fail(e);
      }
      return;
    }
    case "suspend": {
      const suspend = !row?.cells.suspend;
      try {
        await api.suspendCronjob(clusterId, namespace!, name, suspend);
        ok(suspend ? `CronJob ${name} suspended` : `CronJob ${name} resumed`);
      } catch (e) {
        fail(e);
      }
      return;
    }
    case "trigger":
      try {
        const job = await api.triggerCronjob(clusterId, namespace!, name);
        ok(`Created job ${job}`);
      } catch (e) {
        fail(e);
      }
      return;
    case "logs": {
      if (ref.kind === "Pod") {
        app.openLogs({ clusterId, namespace: namespace!, pods: [name], title: name });
      } else {
        const selector = row?.cells.selector as string | undefined;
        if (!selector) return fail(new Error("This workload has no label selector"));
        try {
          const pods = await api.workloadPods(clusterId, namespace!, selector);
          if (!pods.length) return fail(new Error("No pods found for this workload"));
          app.openLogs({ clusterId, namespace: namespace!, pods: pods.map((p) => p.name).slice(0, 20), title: `${ref.kind} ${name}` });
        } catch (e) {
          fail(e);
        }
      }
      return;
    }
    case "shell": {
      if (ref.kind === "Pod") {
        const containers = (row?.cells.containers as string[] | undefined) ?? [];
        app.openTerminal({ clusterId, namespace: namespace!, pod: name, container: containers[0], shell: "auto" });
      }
      return;
    }
  }
}

export function openBulkDelete(targets: Target[]) {
  useDialogs.getState().open({ type: "delete", targets });
}

function DeleteDialog({ targets, onClose }: { targets: Target[]; onClose: () => void }) {
  const settings = useApp((s) => s.settings);
  const toast = useApp((s) => s.toast);
  const qc = useQueryClient();
  const clusterId = targets[0].clusterId;
  const profile = clusterOf(clusterId);
  const strict = profile?.environment === "production" && (settings?.strictProductionConfirmations ?? true);
  const single = targets.length === 1;
  const expected = single ? targets[0].name : `delete ${targets.length}`;
  const [typed, setTyped] = useState("");
  const [propagation, setPropagation] = useState("Background");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const go = async () => {
    setBusy(true);
    setError(null);
    const failures: string[] = [];
    for (const t of targets) {
      try {
        await api.deleteResource(t.clusterId, t.ref, t.namespace, t.name, { propagation }, strict ? t.name : undefined);
      } catch (e) {
        failures.push(`${t.name}: ${errorMessage(e)}`);
      }
    }
    setBusy(false);
    qc.invalidateQueries();
    if (failures.length) {
      setError(failures.join("\n"));
    } else {
      toast({ kind: "success", title: single ? `Deleted ${targets[0].ref.kind.toLowerCase()} ${targets[0].name}` : `Deleted ${targets.length} objects` });
      if (useApp.getState().selection && targets.some((t) => t.name === useApp.getState().selection?.name)) useApp.getState().select(null);
      onClose();
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={single ? `Delete ${targets[0].ref.kind} “${targets[0].name}”?` : `Delete ${targets.length} objects?`}
      description="This operation cannot be undone."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" loading={busy} disabled={strict && typed !== expected} onClick={go}>
            Delete
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <TargetCluster clusterId={clusterId} />
        {!single && (
          <ul className="selectable max-h-40 overflow-auto rounded-md border border-border px-3 py-2 font-mono text-[12px]">
            {targets.map((t) => (
              <li key={`${t.namespace}/${t.name}`}>
                {t.namespace ? `${t.namespace}/` : ""}
                {t.name}
              </li>
            ))}
          </ul>
        )}
        {single && targets[0].namespace && (
          <div className="text-[12.5px] text-muted">
            Namespace <span className="font-medium text-fg">{targets[0].namespace}</span>
          </div>
        )}
        <Field label="Propagation" hint="How dependent objects (e.g. pods of a deployment) are handled.">
          <Select
            value={propagation}
            onChange={setPropagation}
            options={[
              { value: "Background", label: "Background – delete dependents asynchronously" },
              { value: "Foreground", label: "Foreground – delete dependents first" },
              { value: "Orphan", label: "Orphan – keep dependents" },
            ]}
          />
        </Field>
        {strict && (
          <div className="flex flex-col gap-2 rounded-md border border-err/30 bg-err-soft p-3">
            <div className="flex items-center gap-2 text-[12.5px] font-medium text-err">
              <AlertTriangle className="size-4" /> Production cluster
            </div>
            <div className="text-[12.5px] text-fg">
              Type <span className="font-mono font-semibold">{expected}</span> to confirm.
            </div>
            <Input autoFocus value={typed} onChange={(e) => setTyped(e.target.value)} />
          </div>
        )}
        {error && <ErrorBanner message={error} />}
      </div>
    </Dialog>
  );
}

function ScaleDialog({ target, current, onClose }: { target: Target; current: number; onClose: () => void }) {
  const [replicas, setReplicas] = useState(String(current));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toast = useApp((s) => s.toast);
  const n = Number(replicas);
  const valid = Number.isInteger(n) && n >= 0 && n <= 10000;
  const go = async () => {
    setBusy(true);
    try {
      await api.scaleResource(target.clusterId, target.ref, target.namespace!, target.name, n);
      toast({ kind: "success", title: `Scaled ${target.name} to ${n}` });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`Scale ${target.ref.kind} “${target.name}”`}
      width={420}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!valid || n === current} onClick={go}>
            Scale
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) go();
        }}
      >
        <TargetCluster clusterId={target.clusterId} />
        <Field label="Replicas" hint={`Currently ${current}`} error={valid ? null : "Enter a number between 0 and 10000"}>
          <div className="flex items-center gap-2">
            <Button type="button" size="icon" onClick={() => setReplicas(String(Math.max(0, n - 1)))}>
              −
            </Button>
            <Input autoFocus className="w-24" value={replicas} onChange={(e) => setReplicas(e.target.value)} inputMode="numeric" />
            <Button type="button" size="icon" onClick={() => setReplicas(String(n + 1))}>
              +
            </Button>
          </div>
        </Field>
        {n === 0 && current > 0 && <div className="text-[12.5px] text-warn">Scaling to zero stops all pods of this workload.</div>}
        {error && <ErrorBanner message={error} />}
      </form>
    </Dialog>
  );
}

function HistoryDialog({ target, onClose }: { target: Target; onClose: () => void }) {
  const [revs, setRevs] = useState<RolloutRevision[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const toast = useApp((s) => s.toast);
  const load = () =>
    api
      .rolloutHistory(target.clusterId, target.namespace!, target.name)
      .then(setRevs)
      .catch((e) => setError(errorMessage(e)));
  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const rollback = async (rev: number) => {
    setBusy(rev);
    try {
      await api.rollbackDeployment(target.clusterId, target.namespace!, target.name, rev);
      toast({ kind: "success", title: `Rolled back ${target.name} to revision ${rev}` });
      await load();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={`Rollout history – ${target.name}`} width={720}>
      <div className="flex flex-col gap-3">
        <TargetCluster clusterId={target.clusterId} />
        {error && <ErrorBanner message={error} />}
        {!revs && !error && <Spinner />}
        {revs && (
          <div className="overflow-hidden rounded-md border border-border">
            {revs.map((r) => (
              <div key={r.revision} className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-0">
                <span className="tabular w-10 font-semibold">#{r.revision}</span>
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate font-mono text-[12px]">{r.images.join(", ")}</span>
                  <span className="text-[12px] text-muted">
                    {r.replicaSet} · {age(r.created)} ago {r.changeCause ? `· ${r.changeCause}` : ""}
                  </span>
                </div>
                {r.current ? (
                  <Badge tone="ok">current</Badge>
                ) : (
                  <Button size="sm" icon={<RotateCcw className="size-3.5" />} loading={busy === r.revision} onClick={() => rollback(r.revision)}>
                    Roll back
                  </Button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </Dialog>
  );
}

function DrainDialog({ target, onClose }: { target: Target; onClose: () => void }) {
  const settings = useApp((s) => s.settings);
  const profile = clusterOf(target.clusterId);
  const strict = profile?.environment === "production" && (settings?.strictProductionConfirmations ?? true);
  const [opts, setOpts] = useState({ ignoreDaemonsets: true, deleteEmptydirData: false, force: false });
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<DrainReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await api.drainNode(target.clusterId, target.name, opts, strict ? target.name : undefined));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`Drain node “${target.name}”`}
      description="The node is cordoned and its pods are evicted, respecting PodDisruptionBudgets."
      footer={
        report ? (
          <Button onClick={onClose}>Close</Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button variant="danger" loading={busy} disabled={strict && typed !== target.name} onClick={go}>
              Drain node
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-3">
        <TargetCluster clusterId={target.clusterId} />
        {!report && (
          <>
            <Toggle checked={opts.ignoreDaemonsets} onChange={(v) => setOpts({ ...opts, ignoreDaemonsets: v })} label="Ignore DaemonSet pods" />
            <Toggle checked={opts.deleteEmptydirData} onChange={(v) => setOpts({ ...opts, deleteEmptydirData: v })} label="Delete emptyDir data" description="Pods using emptyDir volumes lose that data." />
            <Toggle checked={opts.force} onChange={(v) => setOpts({ ...opts, force: v })} label="Force" description="Also evict pods that are not managed by a controller." />
            {strict && (
              <Field label={`Type ${target.name} to confirm`}>
                <Input value={typed} onChange={(e) => setTyped(e.target.value)} />
              </Field>
            )}
          </>
        )}
        {report && (
          <div className="selectable flex flex-col gap-2 text-[12.5px]">
            <div>
              <span className="font-medium text-ok">{report.evicted.length} evicted</span> · {report.skipped.length} skipped ·{" "}
              <span className={report.failed.length ? "font-medium text-err" : ""}>{report.failed.length} failed</span>
            </div>
            {report.failed.map(([p, why]) => (
              <div key={p} className="text-err">
                {p}: {why}
              </div>
            ))}
            {report.skipped.map(([p, why]) => (
              <div key={p} className="text-muted">
                {p}: {why}
              </div>
            ))}
          </div>
        )}
        {error && <ErrorBanner message={error} />}
      </div>
    </Dialog>
  );
}

export function ActionDialogs() {
  const { dialog, open } = useDialogs();
  const close = () => open(null);
  if (!dialog) return null;
  switch (dialog.type) {
    case "delete":
      return <DeleteDialog targets={dialog.targets} onClose={close} />;
    case "scale":
      return <ScaleDialog target={dialog.target} current={dialog.current} onClose={close} />;
    case "history":
      return <HistoryDialog target={dialog.target} onClose={close} />;
    case "drain":
      return <DrainDialog target={dialog.target} onClose={close} />;
  }
}

export const ACTION_LABELS: Record<ActionId, string> = {
  logs: "Logs",
  shell: "Shell",
  scale: "Scale…",
  restart: "Restart",
  pause: "Pause / resume rollout",
  history: "Rollout history…",
  cordon: "Cordon / uncordon",
  drain: "Drain…",
  trigger: "Run now",
  suspend: "Suspend / resume",
  edit: "Edit YAML",
  delete: "Delete…",
};
