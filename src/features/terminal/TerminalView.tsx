import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { Copy, Plus, SquareTerminal, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, Dialog, EmptyState, Field, Select, Tooltip } from "@/components/ui";
import { useResolvedTheme } from "@/hooks/useTheme";
import { useWatch } from "@/hooks/useWatch";
import { cn } from "@/lib/format";
import { api, Channel, errorMessage } from "@/lib/ipc";
import type { ExecEvent } from "@/lib/types";
import { KINDS } from "@/features/resources/catalog";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useApp, type TerminalTab } from "@/store/app";

const SHELLS = [
  { value: "auto", label: "Auto (bash → ash → sh)" },
  { value: "bash", label: "/bin/bash" },
  { value: "sh", label: "/bin/sh" },
  { value: "ash", label: "/bin/ash" },
  { value: "zsh", label: "/bin/zsh" },
  { value: "powershell", label: "PowerShell (Windows)" },
  { value: "cmd", label: "cmd.exe (Windows)" },
];

const darkTheme = {
  background: "#0e0f13",
  foreground: "#e8e8ee",
  cursor: "#6d7bff",
  selectionBackground: "#2d3570",
  black: "#1b1c23",
  brightBlack: "#6c6c7a",
  red: "#f26565",
  green: "#3ccf7a",
  yellow: "#e7a93b",
  blue: "#6d7bff",
  magenta: "#c084fc",
  cyan: "#38bdf8",
  white: "#e8e8ee",
};
const lightTheme = {
  background: "#ffffff",
  foreground: "#17171c",
  cursor: "#4f5ff0",
  selectionBackground: "#d9ddff",
  black: "#17171c",
  red: "#dc2626",
  green: "#16a34a",
  yellow: "#a16207",
  blue: "#4f5ff0",
  magenta: "#9333ea",
  cyan: "#0891b2",
  white: "#e3e3e8",
};

function decode(payload: unknown): Uint8Array {
  if (payload instanceof ArrayBuffer) return new Uint8Array(payload);
  if (ArrayBuffer.isView(payload)) return new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
  if (Array.isArray(payload)) return Uint8Array.from(payload);
  if (typeof payload === "string") return new TextEncoder().encode(payload);
  return new Uint8Array();
}

/** One interactive exec session rendered by xterm.js. */
function TerminalSession({ tab, visible }: { tab: TerminalTab; visible: boolean }) {
  const el = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionRef = useRef<string | null>(null);
  const [status, setStatus] = useState<"connecting" | "open" | "closed">("connecting");
  const theme = useResolvedTheme();
  const fontSize = useApp((s) => s.settings?.terminalFontSize ?? 13);
  const cluster = useApp((s) => s.clusters.find((c) => c.profile.id === tab.clusterId));

  useEffect(() => {
    const term = new Terminal({
      fontFamily: "JetBrains Mono Variable, ui-monospace, monospace",
      fontSize,
      cursorBlink: true,
      scrollback: 10_000,
      allowProposedApi: false,
      theme: theme === "dark" ? darkTheme : lightTheme,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(el.current!);
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;
    term.writeln(`\x1b[2mConnecting to ${tab.namespace}/${tab.pod}${tab.container ? ` (${tab.container})` : ""} on ${cluster?.profile.name ?? tab.clusterId}…\x1b[0m`);

    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (buf) => term.write(decode(buf));
    const onEvent = new Channel<ExecEvent>();
    onEvent.onmessage = (ev) => {
      setStatus("closed");
      if (ev.type === "exit") term.writeln(`\r\n\x1b[2m[session ended${ev.code != null ? ` with exit code ${ev.code}` : ""}]\x1b[0m`);
      else term.writeln(`\r\n\x1b[31m[${ev.message}]\x1b[0m`);
    };
    let disposed = false;
    api
      .execStart(tab.clusterId, tab.namespace, tab.pod, tab.container, tab.shell, term.cols, term.rows, onData as any, onEvent)
      .then((id) => {
        if (disposed) {
          api.execClose(id).catch(() => {});
          return;
        }
        sessionRef.current = id;
        setStatus("open");
        term.focus();
      })
      .catch((e) => {
        setStatus("closed");
        term.writeln(`\r\n\x1b[31mCould not start shell: ${errorMessage(e)}\x1b[0m`);
        if (tab.shell === "auto") term.writeln("\x1b[2mThe container may not include a shell (e.g. distroless images).\x1b[0m");
      });
    const input = term.onData((d) => {
      if (sessionRef.current) api.execWrite(sessionRef.current, d).catch(() => {});
    });
    const resize = term.onResize(({ cols, rows }) => {
      if (sessionRef.current) api.execResize(sessionRef.current, cols, rows).catch(() => {});
    });
    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* hidden */
      }
    });
    ro.observe(el.current!);
    return () => {
      disposed = true;
      ro.disconnect();
      input.dispose();
      resize.dispose();
      if (sessionRef.current) api.execClose(sessionRef.current).catch(() => {});
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab.id]);

  useEffect(() => {
    if (termRef.current) {
      termRef.current.options.theme = theme === "dark" ? darkTheme : lightTheme;
      termRef.current.options.fontSize = fontSize;
      fitRef.current?.fit();
    }
  }, [theme, fontSize]);

  useEffect(() => {
    if (visible) {
      requestAnimationFrame(() => {
        fitRef.current?.fit();
        termRef.current?.focus();
      });
    }
  }, [visible]);

  const copyAll = () => {
    const t = termRef.current;
    if (!t) return;
    const sel = t.getSelection();
    let text = sel;
    if (!text) {
      const buf = t.buffer.active;
      const lines: string[] = [];
      for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? "");
      text = lines.join("\n").trimEnd();
    }
    navigator.clipboard.writeText(text).then(() => useApp.getState().toast({ kind: "info", title: sel ? "Selection copied" : "Terminal output copied" }));
  };

  return (
    <div className={cn("absolute inset-0 flex flex-col", !visible && "invisible")}>
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border bg-panel px-3 text-[12px] text-muted">
        <span className={cn("size-1.5 rounded-full", status === "open" ? "bg-ok" : status === "connecting" ? "bg-pending" : "bg-neutral")} />
        <span className="font-medium text-fg">{cluster?.profile.name}</span>
        <span>
          {tab.namespace}/{tab.pod}
          {tab.container ? ` · ${tab.container}` : ""} · {tab.shell}
        </span>
        <span className="flex-1" />
        <Tooltip content="Copy selection or entire output">
          <Button size="icon-sm" variant="ghost" onClick={copyAll}>
            <Copy className="size-3.5" />
          </Button>
        </Tooltip>
      </div>
      <div className="min-h-0 flex-1 bg-[var(--canvas)] p-2">
        <div ref={el} className="h-full w-full" />
      </div>
    </div>
  );
}

function NewSessionDialog({ onClose }: { onClose: () => void }) {
  const clusterId = useApp((s) => s.activeClusterId);
  const ns = useApp((s) => (s.activeClusterId ? s.namespaces[s.activeClusterId] ?? "" : ""));
  const openTerminal = useApp((s) => s.openTerminal);
  const pods = useWatch(clusterId, KINDS.pods.ref, ns || null);
  const running = useMemo(() => pods.rows.filter((p) => p.status === "Running").sort((a, b) => a.name.localeCompare(b.name)), [pods.rows]);
  const [pod, setPod] = useState("");
  const [container, setContainer] = useState("");
  const [shell, setShell] = useState("auto");
  const row = running.find((p) => `${p.namespace}/${p.name}` === pod);
  const containers = (row?.cells.containers as string[]) ?? [];
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title="New terminal session"
      description={ns ? `Running pods in ${ns}` : "Running pods in all namespaces"}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!row}
            onClick={() => {
              openTerminal({ clusterId: clusterId!, namespace: row!.namespace!, pod: row!.name, container: container || containers[0], shell });
              onClose();
            }}
          >
            Open shell
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Field label="Pod">
          <Select value={pod} onChange={(v) => { setPod(v); setContainer(""); }} options={[{ value: "", label: "Select a running pod…" }, ...running.map((p) => ({ value: `${p.namespace}/${p.name}`, label: `${p.namespace}/${p.name}` }))]} />
        </Field>
        <div className="grid grid-cols-2 gap-4">
          <Field label="Container">
            <Select value={container || containers[0] || ""} onChange={setContainer} options={containers.map((c) => ({ value: c, label: c }))} disabled={!containers.length} />
          </Field>
          <Field label="Shell">
            <Select value={shell} onChange={setShell} options={SHELLS} />
          </Field>
        </div>
      </div>
    </Dialog>
  );
}

export function TerminalView() {
  const terminals = useApp((s) => s.terminals);
  const activeId = useApp((s) => s.activeTerminal);
  const setActive = useApp((s) => s.setActiveTerminal);
  const closeTerminal = useApp((s) => s.closeTerminal);
  const clusters = useApp((s) => s.clusters);
  const activeCluster = useApp((s) => s.activeClusterId);
  const [newOpen, setNewOpen] = useState(false);
  const connected = clusters.find((c) => c.profile.id === activeCluster)?.status?.state === "connected";

  return (
    <div className="flex h-full flex-col">
      <ViewHeader
        title="Terminal"
        description="Interactive shells inside containers via the Kubernetes exec API."
        actions={
          <Button variant="primary" icon={<Plus className="size-4" />} disabled={!connected} onClick={() => setNewOpen(true)}>
            New session
          </Button>
        }
      >
        {terminals.length > 0 && (
          <div className="flex items-center gap-1 overflow-x-auto">
            {terminals.map((t) => {
              const cl = clusters.find((c) => c.profile.id === t.clusterId);
              return (
                <div
                  key={t.id}
                  className={cn(
                    "group flex h-8 shrink-0 items-center gap-2 rounded-md pr-1 pl-2.5 text-[12.5px]",
                    t.id === activeId ? "bg-hover font-medium text-fg" : "text-muted hover:bg-hover/60",
                  )}
                >
                  <button className="flex items-center gap-2" onClick={() => setActive(t.id)}>
                    <SquareTerminal className="size-3.5" />
                    <span className="max-w-56 truncate">{t.pod}</span>
                    {cl?.profile.environment === "production" && <Badge tone="error">prod</Badge>}
                  </button>
                  <button className="rounded p-0.5 text-subtle hover:bg-active hover:text-fg" onClick={() => closeTerminal(t.id)} aria-label="Close session">
                    <X className="size-3.5" />
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </ViewHeader>
      <div className="relative min-h-0 flex-1">
        {terminals.length === 0 ? (
          <EmptyState
            icon={<SquareTerminal className="size-5" />}
            title="No terminal sessions"
            description="Open a shell from a pod's detail panel, the command palette, or start a new session."
            action={
              <Button variant="primary" disabled={!connected} onClick={() => setNewOpen(true)}>
                New session
              </Button>
            }
          />
        ) : (
          terminals.map((t) => <TerminalSession key={t.id} tab={t} visible={t.id === activeId} />)
        )}
      </div>
      {newOpen && <NewSessionDialog onClose={() => setNewOpen(false)} />}
    </div>
  );
}
