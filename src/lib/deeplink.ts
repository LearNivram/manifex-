import { VIEW_IDS, type ViewId } from "@/store/app";

export interface DeepLinkTarget {
  clusterId?: string;
  view?: ViewId;
  namespace?: string;
  kind?: string;
  name?: string;
}

const SAFE = /^[A-Za-z0-9._:-]{1,253}$/;

/**
 * Parses `manifex://` URLs. Supported forms:
 *   manifex://cluster/<id>[/<view>][?namespace=<ns>&kind=<pods>&name=<obj>]
 *   manifex://view/<view>
 * Everything is validated against allow-lists; unknown input yields null.
 */
export function parseDeepLink(raw: string): DeepLinkTarget | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  if (url.protocol !== "manifex:") return null;
  // `manifex://cluster/abc` → host "cluster", pathname "/abc"
  const parts = [url.hostname, ...url.pathname.split("/")].filter(Boolean).map(decodeURIComponent);
  const out: DeepLinkTarget = {};
  if (parts[0] === "cluster" && parts[1]) {
    if (!SAFE.test(parts[1])) return null;
    out.clusterId = parts[1];
    if (parts[2]) {
      if (!VIEW_IDS.includes(parts[2] as ViewId)) return null;
      out.view = parts[2] as ViewId;
    }
  } else if (parts[0] === "view" && parts[1]) {
    if (!VIEW_IDS.includes(parts[1] as ViewId)) return null;
    out.view = parts[1] as ViewId;
  } else {
    return null;
  }
  for (const [k, v] of url.searchParams) {
    if (!SAFE.test(v)) continue;
    if (k === "namespace" || k === "ns") out.namespace = v;
    if (k === "kind") out.kind = v;
    if (k === "name") out.name = v;
  }
  return out;
}
