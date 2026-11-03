import { useEffect, useMemo, useRef, useState } from "react";
import { api, Channel, errorMessage } from "@/lib/ipc";
import type { ResourceRef, Row, WatchMessage } from "@/lib/types";

export interface WatchState {
  rows: Row[];
  ready: boolean;
  error: string | null;
  forbidden: boolean;
}

/** Applies a watch message to a row map; returns true if anything changed. Exported for tests. */
export function applyWatchMessage(map: Map<string, Row>, msg: WatchMessage): boolean {
  switch (msg.type) {
    case "snapshot":
      map.clear();
      for (const r of msg.rows) map.set(r.uid, r);
      return true;
    case "batch":
      for (const r of msg.upserts) map.set(r.uid, r);
      for (const uid of msg.deletes) map.delete(uid);
      return msg.upserts.length > 0 || msg.deletes.length > 0;
    default:
      return false;
  }
}

/**
 * Live view of a Kubernetes collection. The Rust core keeps one shared upstream
 * watch per key and pushes a snapshot followed by batched deltas.
 */
export function useWatch(
  clusterId: string | null | undefined,
  resource: ResourceRef | null | undefined,
  namespace?: string | null,
  opts: { labelSelector?: string; fieldSelector?: string; enabled?: boolean } = {},
): WatchState {
  const [state, setState] = useState<WatchState>({ rows: [], ready: false, error: null, forbidden: false });
  const mapRef = useRef(new Map<string, Row>());
  const enabled = opts.enabled ?? true;
  const key = resource ? `${resource.group}/${resource.version}/${resource.plural}` : "";

  useEffect(() => {
    if (!clusterId || !resource || !enabled) {
      setState({ rows: [], ready: false, error: null, forbidden: false });
      return;
    }
    let cancelled = false;
    let subId: string | null = null;
    const map = mapRef.current;
    map.clear();
    setState({ rows: [], ready: false, error: null, forbidden: false });
    const channel = new Channel<WatchMessage>();
    channel.onmessage = (msg) => {
      if (cancelled) return;
      if (msg.type === "error") {
        setState((s) => ({ ...s, error: msg.message, forbidden: msg.status === 403, ready: s.ready || msg.status === 403 }));
        return;
      }
      if (applyWatchMessage(map, msg) || msg.type === "snapshot") {
        const ready = msg.type === "snapshot" ? msg.ready : true;
        setState((s) => ({ rows: Array.from(map.values()), ready: s.ready || ready, error: ready ? null : s.error, forbidden: false }));
      }
    };
    api
      .watchResources(clusterId, resource, resource.namespaced ? namespace || null : null, channel, opts.labelSelector || null, opts.fieldSelector || null)
      .then((id) => {
        if (cancelled) api.unwatchResources(id).catch(() => {});
        else subId = id;
      })
      .catch((e) => !cancelled && setState({ rows: [], ready: true, error: errorMessage(e), forbidden: false }));
    return () => {
      cancelled = true;
      if (subId) api.unwatchResources(subId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clusterId, key, resource?.namespaced, namespace, opts.labelSelector, opts.fieldSelector, enabled]);

  return state;
}

/** Stable ResourceRef identity for memo deps. */
export function useResourceRef(r: ResourceRef | null | undefined): ResourceRef | null {
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => r ?? null, [r?.group, r?.version, r?.plural, r?.kind, r?.namespaced]);
}
