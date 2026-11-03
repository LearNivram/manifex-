import { parseAllDocuments, stringify } from "yaml";

export interface YamlIssue {
  message: string;
  line: number; // 1-based
  column: number; // 1-based
  severity: "error" | "warning";
}

export interface ManifestInfo {
  apiVersion?: string;
  kind?: string;
  name?: string;
  namespace?: string;
}

/** Parses (multi-document) YAML and returns syntax / structural issues with positions. */
export function lintManifests(text: string): { issues: YamlIssue[]; docs: ManifestInfo[] } {
  const issues: YamlIssue[] = [];
  const docs: ManifestInfo[] = [];
  const parsed = parseAllDocuments(text, { prettyErrors: true });
  const all = Array.isArray(parsed) ? parsed : [parsed];
  for (const doc of all) {
    for (const e of doc.errors) {
      issues.push({ message: e.message.split("\n")[0], line: e.linePos?.[0]?.line ?? 1, column: e.linePos?.[0]?.col ?? 1, severity: "error" });
    }
    for (const w of doc.warnings) {
      issues.push({ message: w.message.split("\n")[0], line: w.linePos?.[0]?.line ?? 1, column: w.linePos?.[0]?.col ?? 1, severity: "warning" });
    }
    if (doc.errors.length) continue;
    const js = doc.toJS();
    if (js === null || js === undefined) continue;
    const startLine = lineOf(text, doc.range?.[0] ?? 0);
    if (typeof js !== "object" || Array.isArray(js)) {
      issues.push({ message: "A Kubernetes manifest must be a mapping", line: startLine, column: 1, severity: "error" });
      continue;
    }
    const info: ManifestInfo = { apiVersion: js.apiVersion, kind: js.kind, name: js.metadata?.name, namespace: js.metadata?.namespace };
    if (!info.apiVersion) issues.push({ message: "apiVersion is required", line: startLine, column: 1, severity: "error" });
    if (!info.kind) issues.push({ message: "kind is required", line: startLine, column: 1, severity: "error" });
    if (!info.name && !String(info.kind ?? "").endsWith("List"))
      issues.push({ message: "metadata.name is required", line: startLine, column: 1, severity: "error" });
    docs.push(info);
  }
  return { issues, docs };
}

function lineOf(text: string, offset: number): number {
  let line = 1;
  for (let i = 0; i < offset && i < text.length; i++) if (text.charCodeAt(i) === 10) line++;
  // skip leading document separator / comments
  return line;
}

/** Normalises formatting (2-space indent, stable quoting) while keeping comments. */
export function formatYaml(text: string): string {
  const docs = parseAllDocuments(text);
  const all = Array.isArray(docs) ? docs : [docs];
  if (all.some((d) => d.errors.length)) throw new Error("Fix syntax errors before formatting");
  return all
    .filter((d) => d.contents !== null)
    .map((d) => d.toString({ indent: 2, lineWidth: 0 }).replace(/^---\n/, ""))
    .join("---\n");
}

export function toYaml(value: unknown): string {
  return stringify(value, { indent: 2, lineWidth: 0 });
}
