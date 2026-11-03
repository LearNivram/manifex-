// Full-screen YAML editor: schema-aware completion & validation (schemas come
// from the connected cluster's OpenAPI v3 document, including CRDs), formatting,
// server-side dry-run validation, diff against the live object and apply.

import Editor, { DiffEditor, type OnMount } from "@monaco-editor/react";
import { useQuery } from "@tanstack/react-query";
import { AlignLeft, CheckCircle2, FileDiff, Play, ShieldCheck, X, XCircle } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, ErrorBanner, Spinner, Toggle } from "@/components/ui";
import { cn } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { ApplyResult } from "@/lib/types";
import { formatYaml, lintManifests } from "@/lib/yamlTools";
import { useApp, type EditorState } from "@/store/app";
import { EnvBadge } from "@/features/resources/actions";
import { monaco, setModelSchema } from "./monaco";
import { useResolvedTheme } from "@/hooks/useTheme";

let modelSeq = 0;

export default function EditorPanel({ state }: { state: EditorState }) {
  const close = () => useApp.getState().openEditor(null);
  const toast = useApp((s) => s.toast);
  const settings = useApp((s) => s.settings);
  const theme = useResolvedTheme();
  const cluster = useApp((s) => s.clusters.find((c) => c.profile.id === state.clusterId));
  const modelPath = useMemo(() => `file:///manifex/${++modelSeq}.yaml`, []);
  const [text, setText] = useState<string>(state.mode === "create" ? state.initial ?? "" : "");
  const [original, setOriginal] = useState<string>("");
  const [loading, setLoading] = useState(state.mode === "edit");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"dry" | "apply" | null>(null);
  const [results, setResults] = useState<ApplyResult[] | null>(null);
  const [diff, setDiff] = useState<{ left: string; right: string } | null>(null);
  const [forceConflicts, setForceConflicts] = useState(false);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);

  useEffect(() => {
    if (state.mode !== "edit") return;
    api
      .getResource(state.clusterId, state.resource, state.namespace, state.name)
      .then((v) => {
        setText(v.yaml);
        setOriginal(v.yaml);
      })
      .catch((e) => setLoadError(errorMessage(e)))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const lint = useMemo(() => lintManifests(text), [text]);
  const first = lint.docs[0];

  // Cluster-provided schema for the first document's kind.
  const schema = useQuery({
    queryKey: ["schema", state.clusterId, first?.apiVersion, first?.kind],
    enabled: !!first?.apiVersion && !!first?.kind,
    staleTime: Infinity,
    retry: false,
    queryFn: () => api.resourceSchema(state.clusterId, first!.apiVersion!, first!.kind!),
  });

  useEffect(() => {
    setModelSchema(modelPath, `${state.clusterId}/${first?.apiVersion}/${first?.kind}`, schema.data ?? null);
  }, [schema.data, modelPath, state.clusterId, first?.apiVersion, first?.kind]);
  useEffect(() => () => setModelSchema(modelPath, "", null), [modelPath]);

  // Syntax markers from the local linter (schema markers come from monaco-yaml).
  useEffect(() => {
    const model = monaco.editor.getModel(monaco.Uri.parse(modelPath));
    if (!model) return;
    monaco.editor.setModelMarkers(
      model,
      "manifex",
      lint.issues.map((i) => ({
        startLineNumber: i.line,
        startColumn: i.column,
        endLineNumber: i.line,
        endColumn: model.getLineMaxColumn(Math.min(i.line, model.getLineCount())),
        message: i.message,
        severity: i.severity === "error" ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
      })),
    );
  }, [lint, modelPath]);

  const hasErrors = lint.issues.some((i) => i.severity === "error");
  const mode = state.mode === "edit" ? "replace" : "apply";

  const run = async (dryRun: boolean) => {
    setBusy(dryRun ? "dry" : "apply");
    setResults(null);
    try {
      const res = await api.applyYaml(state.clusterId, text, { namespace: state.namespace, mode, dryRun, forceConflicts });
      setResults(res);
      const failed = res.filter((r) => r.error);
      if (dryRun) {
        if (!failed.length) {
          const left = res.map((r) => r.previousYaml ?? "").join("---\n");
          const right = res.map((r) => r.yaml ?? "").join("---\n");
          setDiff({ left: state.mode === "edit" ? original : left, right });
        }
      } else if (!failed.length) {
        toast({ kind: "success", title: res.length === 1 ? `${res[0].kind} ${res[0].name} ${res[0].action}` : `${res.length} objects applied` });
        if (state.mode === "edit") {
          const fresh = res[0].yaml ?? text;
          setOriginal(fresh);
          setText(fresh);
          setDiff(null);
        } else {
          close();
        }
      }
    } catch (e) {
      setResults([{ apiVersion: "", kind: "", name: "", action: "error", dryRun, error: errorMessage(e) }]);
    } finally {
      setBusy(null);
    }
  };

  const format = () => {
    try {
      setText(formatYaml(text));
    } catch (e) {
      toast({ kind: "error", title: errorMessage(e) });
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector("[role=dialog]")) close();
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (!hasErrors && !busy) run(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const options = {
    fontFamily: "JetBrains Mono Variable, ui-monospace, monospace",
    fontSize: settings?.editorFontSize ?? 13,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    tabSize: 2,
    renderWhitespace: "boundary" as const,
    automaticLayout: true,
    padding: { top: 12 },
    quickSuggestions: { other: true, comments: false, strings: true },
  };

  const title =
    state.mode === "edit" ? (
      <>
        Edit <span className="text-muted">{state.resource.kind}</span> {state.name}
      </>
    ) : (
      "Create resources"
    );

  return (
    <div className="fixed inset-0 z-30 flex animate-fade-in flex-col bg-canvas">
      <div className="flex h-12 shrink-0 items-center gap-3 border-b border-border bg-panel px-4" data-tauri-drag-region>
        <div className="flex min-w-0 items-center gap-2 text-[14px] font-semibold">{title}</div>
        {state.mode === "edit" && state.namespace && <Badge>{state.namespace}</Badge>}
        <div className="flex items-center gap-2 rounded-md border border-border px-2 py-1 text-[12px]">
          <span className="text-muted">Target</span>
          <span className="font-medium">{cluster?.profile.name}</span>
          {cluster && <EnvBadge environment={cluster.profile.environment} />}
        </div>
        <div className="flex-1" />
        <span className="text-[12px] text-muted">
          {schema.isFetching ? "Loading schema…" : schema.data ? `Schema: ${first?.kind} (${first?.apiVersion})` : first?.kind ? "No schema available" : ""}
        </span>
        <Button size="sm" variant="ghost" icon={<AlignLeft className="size-3.5" />} onClick={format} disabled={hasErrors}>
          Format
        </Button>
        <Button size="sm" icon={<ShieldCheck className="size-3.5" />} loading={busy === "dry"} disabled={hasErrors || !!busy || !text.trim()} onClick={() => run(true)}>
          Validate & diff
        </Button>
        <Button size="sm" variant="primary" icon={<Play className="size-3.5" />} loading={busy === "apply"} disabled={hasErrors || !!busy || !text.trim()} onClick={() => run(false)}>
          {state.mode === "edit" ? "Save" : "Apply"}
        </Button>
        <Button size="icon" variant="ghost" onClick={close} aria-label="Close editor">
          <X className="size-4" />
        </Button>
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">
          {loading ? (
            <div className="flex h-full items-center justify-center">
              <Spinner />
            </div>
          ) : loadError ? (
            <ErrorBanner className="m-4" message={loadError} />
          ) : diff ? (
            <DiffEditor
              original={diff.left}
              modified={diff.right}
              language="yaml"
              theme={theme === "dark" ? "manifex-dark" : "manifex-light"}
              options={{ ...options, readOnly: true, renderSideBySide: true }}
            />
          ) : (
            <Editor
              path={modelPath}
              value={text}
              language="yaml"
              theme={theme === "dark" ? "manifex-dark" : "manifex-light"}
              onChange={(v) => setText(v ?? "")}
              onMount={(ed) => {
                editorRef.current = ed;
                ed.focus();
              }}
              options={options}
            />
          )}
        </div>
        <aside className="flex w-80 shrink-0 flex-col border-l border-border bg-panel">
          <div className="flex h-10 items-center justify-between border-b border-border px-3 text-[12.5px] font-semibold">
            <span>Checks</span>
            {diff && (
              <Button size="sm" variant="ghost" icon={<FileDiff className="size-3.5" />} onClick={() => setDiff(null)}>
                Back to editor
              </Button>
            )}
          </div>
          <div className="flex-1 overflow-auto p-3 text-[12.5px]">
            <div className="mb-3 flex flex-col gap-1.5">
              {lint.issues.length === 0 ? (
                <div className="flex items-center gap-2 text-ok">
                  <CheckCircle2 className="size-4" /> YAML is well-formed ({lint.docs.length} {lint.docs.length === 1 ? "object" : "objects"})
                </div>
              ) : (
                lint.issues.map((i, idx) => (
                  <button
                    key={idx}
                    className={cn("flex items-start gap-2 text-left", i.severity === "error" ? "text-err" : "text-warn")}
                    onClick={() => editorRef.current?.revealLineInCenter(i.line)}
                  >
                    <XCircle className="mt-0.5 size-3.5 shrink-0" />
                    <span>
                      Line {i.line}: {i.message}
                    </span>
                  </button>
                ))
              )}
            </div>
            {results && (
              <div className="flex flex-col gap-2 border-t border-border pt-3">
                <div className="text-[11px] font-medium tracking-wide text-subtle uppercase">{results[0]?.dryRun ? "Server dry run" : "Result"}</div>
                {results.map((r, i) => (
                  <div key={i} className="selectable rounded-md border border-border p-2">
                    <div className="flex items-center gap-2">
                      {r.error ? <XCircle className="size-3.5 text-err" /> : <CheckCircle2 className="size-3.5 text-ok" />}
                      <span className="font-medium">
                        {r.kind} {r.name}
                      </span>
                    </div>
                    {r.error ? (
                      <div className="mt-1 break-words text-err">{r.error}</div>
                    ) : (
                      <div className="mt-1 text-muted">
                        {r.dryRun ? `would be ${r.action}` : r.action}
                        {r.namespace ? ` in ${r.namespace}` : ""}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
            {state.mode === "create" && (
              <div className="mt-4 border-t border-border pt-3">
                <Toggle
                  checked={forceConflicts}
                  onChange={setForceConflicts}
                  label="Force field ownership"
                  description="Take over fields managed by other tools (server-side apply conflicts)."
                />
              </div>
            )}
            <div className="mt-4 border-t border-border pt-3 text-[12px] text-subtle">
              {state.mode === "edit"
                ? "Saving replaces the object with optimistic locking – if someone else changed it meanwhile, the save is rejected."
                : "Objects are created with Kubernetes server-side apply (field manager “manifex”)."}{" "}
              Press ⌘/Ctrl+S to apply.
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}
