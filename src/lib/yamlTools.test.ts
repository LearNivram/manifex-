import { describe, expect, it } from "vitest";
import { formatYaml, lintManifests } from "./yamlTools";

describe("lintManifests", () => {
  it("accepts valid multi-document manifests", () => {
    const { issues, docs } = lintManifests("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a\n---\napiVersion: v1\nkind: Namespace\nmetadata:\n  name: b\n");
    expect(issues).toEqual([]);
    expect(docs.map((d) => d.kind)).toEqual(["ConfigMap", "Namespace"]);
  });

  it("reports syntax errors with line numbers", () => {
    const { issues } = lintManifests("apiVersion: v1\nkind: [unclosed\n");
    expect(issues.length).toBeGreaterThan(0);
    expect(issues[0].severity).toBe("error");
    expect(issues[0].line).toBeGreaterThanOrEqual(2);
  });

  it("reports missing required fields", () => {
    const { issues } = lintManifests("kind: Pod\nmetadata: {}\n");
    const msgs = issues.map((i) => i.message);
    expect(msgs).toContain("apiVersion is required");
    expect(msgs).toContain("metadata.name is required");
  });
});

describe("formatYaml", () => {
  it("normalises indentation and keeps comments", () => {
    const out = formatYaml("a:\n    b: 1 # keep\n");
    expect(out).toBe("a:\n  b: 1 # keep\n");
  });

  it("refuses to format broken YAML", () => {
    expect(() => formatYaml("a: [")).toThrow();
  });
});
