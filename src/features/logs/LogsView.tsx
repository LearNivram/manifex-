import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownToLine, Download, Pause, Play, Search, Trash2, WrapText } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, EmptyState, ErrorBanner, Input, Select, Toggle, Tooltip } from "@/components/ui";
import { useWatch } from "@/hooks/useWatch";
import { cn } from "@/lib/format";
import { api, Channel, errorMessage } from "@/lib/ipc";
import type { LogMessage } from "@/lib/types";
import { KINDS } from "@/features/resources/catalog";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";

const MAX_LINES = 50_000;
const POD_COLORS = ["#6d7bff", "#3ccf7a", "#e7a93b", "#38bdf8", "#c084fc", "#f472b6", "#f26565", "#94a3b8"];

interface Line {
  pod: number;
  text: string;
}

/** Strips ANSI escape sequences for display. */
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]/g;

export function LogsView() {
  const active = useActiveCluster()!;
  const clusterId = active.profile.id;
  const target = useApp((s) => s.logTarget);
  const settings = useApp((s) => s.settings);
  const ns = useNamespace();
  const [namespace, setNamespace] = useState(target?.namespace ?? ns);
  const [pods, setPods] = useState<string[]>(target?.pods ?? []);
  const [container, setContainer] = useState<string>(target?.container ?? "");
  const [follow, setFollow] = useState(true);
  const [timestamps, setTimestamps] = useState(settings?.logTimestamps ?? false);
  const [previous, setPrevious] = useState(false);
  const [tail, setTail] = useState(String(settings?.logTailLines ?? 500));
  const [wrap, setWrap] = useState(false);
  const [filter, setFilter] = useState("");
  const [paused, setPaused] = useState(false);
  const [autoscroll, setAutoscroll] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [ended, setEnded] = useState(false);
  const linesRef = useRef<Line[]>([]);
  const [version, setVersion] = useState(0);
  const pausedRef = useRef(false);
  pausedRef.current = paused;

  useEffect(() => {
    if (target && target.clusterId === clusterId) {
      setNamespace(target.namespace);
      setPods(target.pods);
      setContainer(target.container ?? "");
    }
  }, [target, clusterId]);

  const podWatch = useWatch(clusterId, KINDS.pods.ref, namespace || null, { enabled: !!namespace });
  const podRows = podWatch.rows;
  const containers = useMemo(() => {
    const p = podRows.find((r) => r.name === pods[0]);
    return [...((p?.cells.containers as string[]) ?? []), ...((p?.cells.initContainers as string[]) ?? [])];
  }, [podRows, pods]);

  // Start one stream per selected pod.
  useEffect(() => {
    if (!namespace || !pods.length) return;
    linesRef.current = [];
    setVersion((v) => v + 1);
    setError(null);
    setEnded(false);
    const ids: string[] = [];
    let cancelled = false;
    let pending: Line[] = [];
    const flush = setInterval(() => {
      if (!pending.length || pausedRef.current) return;
      const next = linesRef.current.concat(pending);
      pending = [];
      linesRef.current = next.length > MAX_LINES ? next.slice(next.length - MAX_LINES) : next;
      setVersion((v) => v + 1);
    }, 120);
    pods.forEach((pod, idx) => {
      const ch = new Channel<LogMessage>();
      ch.onmessage = (m) => {
        if (m.type === "lines") for (const l of m.lines) pending.push({ pod: idx, text: l.replace(ANSI, "") });
        else if (m.type === "error") setError(m.message);
        else if (m.type === "end" && pods.length === 1) setEnded(true);
      };
      const n = Number(tail);
      api
        .logsStart(
          clusterId,
          namespace,
          pod,
          { container: container || undefined, follow, tailLines: Number.isFinite(n) && n > 0 ? n : undefined, timestamps, previous },
          ch,
        )
        .then((id) => {
          if (cancelled) api.logsStop(id).catch(() => {});
          else ids.push(id);
        })
        .catch((e) => setError(`${pod}: ${errorMessage(e)}`));
    });
    return () => {
      cancelled = true;
      clearInterval(flush);
      ids.forEach((id) => api.logsStop(id).catch(() => {}));
    };
  }, [clusterId, namespace, pods.join(","), container, follow, timestamps, previous, tail]);

  const visible = useMemo(() => {
    const all = linesRef.current;
    if (!filter) return all;
    let re: RegExp | null = null;
    try {
      re = new RegExp(filter, "i");
    } catch {
      re = null;
    }
    const f = filter.toLowerCase();
    return all.filter((l) => (re ? re.test(l.text) : l.text.toLowerCase().includes(f)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, filter]);

  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 19,
    overscan: 30,
    measureElement: wrap ? (el) => el.getBoundingClientRect().height : undefined,
  });
  useEffect(() => {
    if (autoscroll && visible.length) virtualizer.scrollToIndex(visible.length - 1, { align: "end" });
  }, [visible.length, autoscroll, virtualizer]);

  const download = async () => {
    const content = linesRef.current.map((l) => (pods.length > 1 ? `[${pods[l.pod]}] ${l.text}` : l.text)).join("\n");
    const path = await api.saveTextFile(`${pods.length === 1 ? pods[0] : "logs"}.log`, content).catch((e) => {
      useApp.getState().toast({ kind: "error", title: errorMessage(e) });
      return null;
    });
    if (path) useApp.getState().toast({ kind: "success", title: "Logs saved", message: path });
  };

  return (
    <div className="flex h-full flex-col">
      <ViewHeader
        title="Logs"
        description={pods.length ? `${pods.length > 1 ? `${pods.length} pods` : pods[0]}${container ? ` · ${container}` : ""} · ${namespace}` : "Stream container logs"}
        actions={
          <>
            <Input icon={<Search className="size-3.5" />} placeholder="Filter (regex)…" value={filter} onChange={(e) => setFilter(e.target.value)} className="w-60" />
            <Tooltip content={paused ? "Resume" : "Pause rendering"}>
              <Button size="icon" variant="ghost" onClick={() => setPaused(!paused)}>
                {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
              </Button>
            </Tooltip>
            <Tooltip content="Wrap lines">
              <Button size="icon" variant={wrap ? "subtle" : "ghost"} onClick={() => setWrap(!wrap)}>
                <WrapText className="size-4" />
              </Button>
            </Tooltip>
            <Tooltip content="Follow output">
              <Button size="icon" variant={autoscroll ? "subtle" : "ghost"} onClick={() => setAutoscroll(!autoscroll)}>
                <ArrowDownToLine className="size-4" />
              </Button>
            </Tooltip>
            <Tooltip content="Clear">
              <Button
                size="icon"
                variant="ghost"
                onClick={() => {
                  linesRef.current = [];
                  setVersion((v) => v + 1);
                }}
              >
                <Trash2 className="size-4" />
              </Button>
            </Tooltip>
            <Button icon={<Download className="size-4" />} onClick={download} disabled={!linesRef.current.length}>
              Save
            </Button>
          </>
        }
      >
        <div className="flex flex-wrap items-center gap-3 pt-1">
          <NamespaceOptions clusterId={clusterId} value={namespace} onChange={(v) => { setNamespace(v); setPods([]); }} />
          <Select
            value={pods.length > 1 ? "__multi" : pods[0] ?? ""}
            onChange={(v) => {
              setPods(v ? [v] : []);
              setContainer("");
            }}
            options={[
              { value: "", label: namespace ? "Select pod…" : "—" },
              ...(pods.length > 1 ? [{ value: "__multi", label: `${pods.length} pods (workload)` }] : []),
              ...podRows.map((p) => ({ value: p.name, label: p.name })),
            ]}
            className="w-72"
          />
          {pods.length === 1 && (
            <Select value={container} onChange={setContainer} options={[{ value: "", label: "Default container" }, ...containers.map((c) => ({ value: c, label: c }))]} className="w-44" />
          )}
          <Select
            value={tail}
            onChange={setTail}
            options={["100", "500", "1000", "5000", "20000", "0"].map((v) => ({ value: v, label: v === "0" ? "All lines" : `Last ${v}` }))}
            className="w-32"
          />
          <Toggle checked={follow} onChange={setFollow} label="Follow" />
          <Toggle checked={timestamps} onChange={setTimestamps} label="Timestamps" />
          <Toggle checked={previous} onChange={setPrevious} label="Previous container" />
          {pods.length > 1 && (
            <div className="flex flex-wrap gap-1">
              {pods.map((p, i) => (
                <Badge key={p}>
                  <span className="size-1.5 rounded-full" style={{ background: POD_COLORS[i % POD_COLORS.length] }} />
                  {p}
                </Badge>
              ))}
            </div>
          )}
        </div>
      </ViewHeader>
      {error && <ErrorBanner className="mx-4 mt-3" message={error} />}
      <div ref={parentRef} className="selectable min-h-0 flex-1 overflow-auto bg-canvas font-mono text-[12px] leading-[19px]" onWheel={(e) => e.deltaY < 0 && setAutoscroll(false)}>
        {!pods.length ? (
          <EmptyState title="Choose a pod" description="Select a namespace and pod, or open logs from a pod, deployment or the command palette." />
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualizer.getVirtualItems().map((vi) => {
              const l = visible[vi.index];
              return (
                <div
                  key={vi.key}
                  data-index={vi.index}
                  ref={wrap ? virtualizer.measureElement : undefined}
                  className={cn("absolute left-0 w-full px-4", wrap ? "break-all whitespace-pre-wrap" : "whitespace-pre")}
                  style={{ transform: `translateY(${vi.start}px)`, height: wrap ? undefined : vi.size }}
                >
                  {pods.length > 1 && (
                    <span className="mr-2" style={{ color: POD_COLORS[l.pod % POD_COLORS.length] }}>
                      {pods[l.pod].slice(-12)}
                    </span>
                  )}
                  <span className={/\b(error|fatal|panic|exception)\b/i.test(l.text) ? "text-err" : /\bwarn(ing)?\b/i.test(l.text) ? "text-warn" : "text-fg"}>{l.text}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
      <div className="flex h-6 shrink-0 items-center gap-3 border-t border-border bg-panel px-4 text-[11.5px] text-muted">
        <span className="tabular">
          {visible.length.toLocaleString()} {filter ? `of ${linesRef.current.length.toLocaleString()} ` : ""}lines
        </span>
        {paused && <span className="text-warn">rendering paused</span>}
        {ended && <span>stream ended</span>}
        {linesRef.current.length >= MAX_LINES && <span>keeping the last {MAX_LINES.toLocaleString()} lines</span>}
      </div>
    </div>
  );
}

function NamespaceOptions({ clusterId, value, onChange }: { clusterId: string; value: string; onChange: (v: string) => void }) {
  const { rows } = useWatch(clusterId, KINDS.namespaces.ref, null);
  if (!rows.length) return null;
  return (
    <select
      aria-label="Namespace list"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="h-8 w-44 rounded-md border border-border bg-panel px-2 text-[13px] focus:border-accent focus:outline-none"
    >
      <option value="">Namespace…</option>
      {rows
        .map((r) => r.name)
        .sort()
        .map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
    </select>
  );
}
