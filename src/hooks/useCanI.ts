import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/ipc";
import type { AccessCheck, ResourceRef } from "@/lib/types";
import type { ActionId } from "@/features/resources/catalog";

export function checkFor(action: ActionId, ref: ResourceRef, namespace?: string | null): AccessCheck | null {
  const ns = ref.namespaced ? namespace || undefined : undefined;
  const base = { group: ref.group, resource: ref.plural, namespace: ns };
  switch (action) {
    case "delete":
      return { ...base, verb: "delete" };
    case "edit":
      return { ...base, verb: "update" };
    case "scale":
    case "restart":
    case "pause":
    case "suspend":
    case "cordon":
      return { ...base, verb: "patch" };
    case "history":
      return { ...base, verb: "get" };
    case "drain":
      return { group: "", resource: "pods", subresource: "eviction", verb: "create" };
    case "logs":
      return { group: "", resource: "pods", subresource: "log", verb: "get", namespace: ns };
    case "shell":
      return { group: "", resource: "pods", subresource: "exec", verb: "create", namespace: ns };
    case "trigger":
      return { group: "batch", resource: "jobs", verb: "create", namespace: ns };
    default:
      return null;
  }
}

/**
 * Asks the API server (SelfSubjectAccessReview) which actions the current identity
 * may perform. Results are cached in the Rust core for 60s. Unknown = allowed; the
 * API server is the final authority either way.
 */
export function useCanI(clusterId: string | null | undefined, ref: ResourceRef | null | undefined, actions: ActionId[], namespace?: string | null) {
  const checks = ref ? actions.map((a) => checkFor(a, ref, namespace)) : [];
  const q = useQuery({
    queryKey: ["can-i", clusterId, ref?.group, ref?.plural, namespace, actions.join(",")],
    enabled: !!clusterId && !!ref,
    staleTime: 60_000,
    queryFn: async () => {
      const real = checks.filter((c): c is AccessCheck => !!c);
      const res = real.length ? await api.canI(clusterId!, real) : [];
      const out: Record<string, boolean> = {};
      let i = 0;
      actions.forEach((a, idx) => {
        out[a] = checks[idx] ? res[i++] ?? true : true;
      });
      return out;
    },
  });
  return (action: ActionId) => q.data?.[action] ?? true;
}
