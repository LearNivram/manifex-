// Compact per-container image/version editor for workloads (kubectl set image).

import { History, Pencil } from "lucide-react";
import { useState } from "react";
import { Button, Input, Tooltip } from "@/components/ui";
import { confirm } from "@/components/ui/confirm";
import { api, errorMessage } from "@/lib/ipc";
import type { ResourceRef } from "@/lib/types";
import { useApp } from "@/store/app";
import { runAction } from "./actions";

interface Container {
  name: string;
  image: string;
  init: boolean;
}

function podSpec(obj: any, kind: string) {
  return kind === "CronJob" ? obj?.spec?.jobTemplate?.spec?.template?.spec : obj?.spec?.template?.spec;
}

/** Splits "repo/name:tag@digest" into repository and tag for a friendlier editor. */
export function splitImage(image: string): { repo: string; tag: string } {
  const at = image.indexOf("@");
  const base = at >= 0 ? image.slice(0, at) : image;
  const slash = base.lastIndexOf("/");
  const colon = base.lastIndexOf(":");
  if (colon > slash) return { repo: base.slice(0, colon), tag: base.slice(colon + 1) };
  return { repo: base, tag: "latest" };
}

function Row({ c, onApply }: { c: Container; onApply: (image: string) => Promise<void> }) {
  const { repo, tag } = splitImage(c.image);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(tag);
  const [busy, setBusy] = useState(false);
  const next = value.includes("/") || value.includes(":") ? value.trim() : `${repo}:${value.trim()}`;
  const changed = value.trim() !== "" && next !== c.image;
  return (
    <div className="rounded-md border border-border px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 flex-col">
          <span className="text-[12px] text-muted">
            {c.name}
            {c.init ? " (init)" : ""}
          </span>
          <span className="selectable truncate font-mono text-[12px]" title={c.image}>
            {repo}
            <span className="font-semibold text-accent">:{tag}</span>
          </span>
        </div>
        {!editing && (
          <Tooltip content="Change version">
            <Button size="icon-sm" variant="ghost" onClick={() => setEditing(true)} aria-label={`Change image of ${c.name}`}>
              <Pencil className="size-3.5" />
            </Button>
          </Tooltip>
        )}
      </div>
      {editing && (
        <form
          className="mt-2 flex items-center gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!changed) return;
            setBusy(true);
            try {
              await onApply(next);
              setEditing(false);
            } finally {
              setBusy(false);
            }
          }}
        >
          <Input autoFocus value={value} onChange={(e) => setValue(e.target.value)} className="flex-1" placeholder="tag (e.g. 1.27.3) or full image" />
          <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!changed}>
            Roll out
          </Button>
          <Button size="sm" variant="ghost" type="button" onClick={() => (setEditing(false), setValue(tag))}>
            Cancel
          </Button>
        </form>
      )}
    </div>
  );
}

export function ImageEditor({ clusterId, resource, namespace, name, obj }: { clusterId: string; resource: ResourceRef; namespace: string; name: string; obj: any }) {
  const toast = useApp((s) => s.toast);
  const cluster = useApp((s) => s.clusters.find((c) => c.profile.id === clusterId));
  const spec = podSpec(obj, resource.kind);
  const containers: Container[] = [
    ...(spec?.containers ?? []).map((c: any) => ({ name: c.name, image: c.image ?? "", init: false })),
    ...(spec?.initContainers ?? []).map((c: any) => ({ name: c.name, image: c.image ?? "", init: true })),
  ];
  if (!containers.length) return null;

  const apply = async (container: string, image: string) => {
    if (cluster?.profile.environment === "production") {
      const ok = await confirm({
        title: `Roll out ${image}?`,
        message: `${resource.kind} ${name} in ${namespace} on production cluster ${cluster.profile.name}. Pods are replaced according to the rollout strategy.`,
        confirmLabel: "Roll out",
      });
      if (!ok) return;
    }
    try {
      const previous = await api.setContainerImage(clusterId, resource, namespace, name, container, image);
      toast({ kind: "success", title: `Rolling out ${image}`, message: `Previously ${previous}. Roll back any time via the rollout history.` });
    } catch (e) {
      toast({ kind: "error", title: "Image update failed", message: errorMessage(e) });
      throw e;
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h4 className="text-[11px] font-semibold tracking-wide text-subtle uppercase">Images / versions</h4>
        {resource.kind === "Deployment" && (
          <Button size="sm" variant="ghost" icon={<History className="size-3.5" />} onClick={() => runAction("history", { clusterId, ref: resource, namespace, name })}>
            History
          </Button>
        )}
      </div>
      {containers.map((c) => (
        <Row key={`${c.init}-${c.name}`} c={c} onApply={(img) => apply(c.name, img)} />
      ))}
    </div>
  );
}
