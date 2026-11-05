import { useQuery } from "@tanstack/react-query";
import { useUpdater } from "@/hooks/useUpdater";
import { CheckCircle2, KeyRound, RefreshCw, ShieldAlert } from "lucide-react";
import React, { useEffect, useState } from "react";
import { Badge, Button, ErrorBanner, Field, Input, KeyValue, Section, Select, Toggle } from "@/components/ui";
import { MOD } from "@/components/layout/TitleBar";
import { api, errorMessage } from "@/lib/ipc";
import type { AppSettings } from "@/lib/types";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useApp } from "@/store/app";

function Row({ title, description, children }: { title: string; description?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-border px-4 py-3 last:border-0">
      <div className="flex min-w-0 flex-col">
        <span className="text-[13px] font-medium">{title}</span>
        {description && <span className="text-[12px] text-muted">{description}</span>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

const SHORTCUTS: [string, string][] = [
  [`${MOD} K`, "Command palette"],
  [`${MOD} ⇧ F`, "Search resources across clusters"],
  [`${MOD} B`, "Toggle sidebar"],
  [`${MOD} ,`, "Settings"],
  [`${MOD} 1…9`, "Switch to cluster 1…9"],
  [`${MOD} ⇧ N`, "New window"],
  [`${MOD} N`, "Create resource"],
  [`${MOD} S`, "Apply in editor"],
  ["Esc", "Close panel / dialog"],
  ["↑ ↓ ⏎", "Navigate tables, open YAML"],
];

export function SettingsView() {
  const settings = useApp((s) => s.settings);
  const setSettings = useApp((s) => s.setSettings);
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo, refetchInterval: 10_000 });
  const [draft, setDraft] = useState<AppSettings | null>(settings);
  const [error, setError] = useState<string | null>(null);
  const updater = useUpdater();
  useEffect(() => setDraft(settings), [settings]);
  if (!draft) return null;

  const save = async (patch: Partial<AppSettings>) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    try {
      setSettings(await api.saveSettings(next));
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="flex h-full flex-col">
      <ViewHeader title="Settings" description="Preferences are stored locally in the application database." />
      <div className="min-h-0 flex-1 overflow-auto p-5">
        <div className="mx-auto flex max-w-3xl flex-col gap-5">
          {error && <ErrorBanner message={error} />}
          <Section title="Appearance">
            <Row title="Theme">
              <Select
                value={draft.theme}
                onChange={(v) => save({ theme: v as AppSettings["theme"] })}
                options={[
                  { value: "system", label: "System" },
                  { value: "dark", label: "Dark" },
                  { value: "light", label: "Light" },
                ]}
                className="w-40"
              />
            </Row>
            <Row title="Density" description="Row height of tables">
              <Select
                value={draft.density}
                onChange={(v) => save({ density: v as AppSettings["density"] })}
                options={[
                  { value: "comfortable", label: "Comfortable" },
                  { value: "compact", label: "Compact" },
                ]}
                className="w-40"
              />
            </Row>
            <Row title="Editor font size">
              <Input type="number" min={9} max={24} className="w-20" value={draft.editorFontSize} onChange={(e) => save({ editorFontSize: Number(e.target.value) })} />
            </Row>
            <Row title="Terminal font size">
              <Input type="number" min={9} max={24} className="w-20" value={draft.terminalFontSize} onChange={(e) => save({ terminalFontSize: Number(e.target.value) })} />
            </Row>
          </Section>

          <Section title="Safety">
            <Row title="Typed confirmation on production clusters" description="Deleting, draining or uninstalling on production clusters requires typing the object name. Enforced by the native core.">
              <Toggle checked={draft.strictProductionConfirmations} onChange={(v) => save({ strictProductionConfirmations: v })} />
            </Row>
            <Row title="Confirm destructive operations" description="Always ask before deleting resources.">
              <Toggle checked={draft.confirmDestructive} onChange={(v) => save({ confirmDestructive: v })} disabled />
            </Row>
          </Section>

          <Section title="Connections">
            <Row title="Automatic reconnect" description="Retry with exponential back-off when a cluster becomes unreachable.">
              <Toggle checked={draft.autoReconnect} onChange={(v) => save({ autoReconnect: v })} />
            </Row>
            <Row title="Health check interval" description="Seconds between API server probes for connected clusters.">
              <Input type="number" min={5} max={300} className="w-20" value={draft.healthIntervalSecs} onChange={(e) => save({ healthIntervalSecs: Number(e.target.value) })} />
            </Row>
          </Section>

          <Section title="Notifications">
            <Row title="Native desktop notifications">
              <Toggle checked={draft.nativeNotifications} onChange={(v) => save({ nativeNotifications: v })} />
            </Row>
            <Row title="Cluster connection lost / restored">
              <Toggle checked={draft.notifyConnectionLoss} onChange={(v) => save({ notifyConnectionLoss: v })} disabled={!draft.nativeNotifications} />
            </Row>
            <Row title="Pod failures" description="CrashLoopBackOff, OOM kills, image pull errors, evictions">
              <Toggle checked={draft.notifyPodFailures} onChange={(v) => save({ notifyPodFailures: v })} disabled={!draft.nativeNotifications} />
            </Row>
            <Row title="Deployment and workload failures">
              <Toggle checked={draft.notifyDeploymentFailures} onChange={(v) => save({ notifyDeploymentFailures: v })} disabled={!draft.nativeNotifications} />
            </Row>
            <Row title="Node problems" description="NotReady, memory / disk / PID pressure">
              <Toggle checked={draft.notifyNodeProblems} onChange={(v) => save({ notifyNodeProblems: v })} disabled={!draft.nativeNotifications} />
            </Row>
            <Row title="All other warning events">
              <Toggle checked={draft.notifyWarningEvents} onChange={(v) => save({ notifyWarningEvents: v })} disabled={!draft.nativeNotifications} />
            </Row>
          </Section>

          <Section title="Logs & tools">
            <Row title="Default log tail" description="Lines loaded when opening logs">
              <Input type="number" min={10} max={50000} className="w-24" value={draft.logTailLines} onChange={(e) => save({ logTailLines: Number(e.target.value) })} />
            </Row>
            <Row title="Show log timestamps">
              <Toggle checked={draft.logTimestamps} onChange={(v) => save({ logTimestamps: v })} />
            </Row>
            <Row title="Helm binary" description="Leave empty to use helm from PATH.">
              <Input className="w-72" placeholder="/usr/local/bin/helm" value={draft.helmPath} onChange={(e) => setDraft({ ...draft, helmPath: e.target.value })} onBlur={() => save({ helmPath: draft.helmPath })} />
            </Row>
          </Section>

          <Section title="Desktop">
            <Row title="Keep running in the system tray" description="Closing the main window hides it instead of quitting.">
              <Toggle checked={draft.closeToTray} onChange={(v) => save({ closeToTray: v })} />
            </Row>
            <Row title="Global shortcut" description="Show / hide Manifex from anywhere. Empty to disable.">
              <Input className="w-56" value={draft.globalShortcut} onChange={(e) => setDraft({ ...draft, globalShortcut: e.target.value })} onBlur={() => save({ globalShortcut: draft.globalShortcut })} />
            </Row>
            <Row title="Check for updates automatically">
              <Toggle checked={draft.autoCheckUpdates} onChange={(v) => save({ autoCheckUpdates: v })} />
            </Row>
            <Row
              title="Updates"
              description={
                updater.status === "available"
                  ? `Version ${updater.update?.version} is available.`
                  : updater.status === "none"
                    ? "You are on the latest version."
                    : updater.status === "downloading"
                      ? `Downloading… ${updater.progress?.toFixed(0) ?? ""}%`
                      : updater.status === "error"
                        ? updater.error
                        : `Installed version ${info.data?.version ?? ""}`
              }
            >
              {updater.status === "available" ? (
                <Button variant="primary" onClick={updater.install}>
                  Install & restart
                </Button>
              ) : (
                <Button icon={<RefreshCw className="size-3.5" />} loading={updater.status === "checking" || updater.status === "downloading"} onClick={() => updater.check()}>
                  Check now
                </Button>
              )}
            </Row>
          </Section>

          <Section title="Keyboard shortcuts">
            <div className="grid grid-cols-2 gap-x-6 gap-y-1.5 p-4 text-[12.5px]">
              {SHORTCUTS.map(([k, v]) => (
                <div key={k} className="flex items-center justify-between gap-3">
                  <span className="text-muted">{v}</span>
                  <span className="font-mono text-[12px]">{k}</span>
                </div>
              ))}
            </div>
          </Section>

          <Section title="About & security">
            <div className="flex flex-col gap-4 p-4">
              {info.data && (
                <>
                  <div className="flex items-center gap-2 text-[12.5px]">
                    {info.data.vaultAvailable ? (
                      <>
                        <CheckCircle2 className="size-4 text-ok" /> Credentials are encrypted with a key stored in the OS credential store.
                      </>
                    ) : (
                      <>
                        <ShieldAlert className="size-4 text-err" /> OS credential store unavailable – tokens and certificates cannot be saved. {info.data.vaultMessage}
                      </>
                    )}
                  </div>
                  <KeyValue
                    items={[
                      ["Version", info.data.version],
                      ["Platform", `${info.data.os} / ${info.data.arch}`],
                      ["Data directory", <span className="font-mono text-[11.5px]">{info.data.dataDir}</span>],
                      ["Active watches", `${info.data.activeWatches} upstream · ${info.data.activeSubscriptions} subscriptions`],
                      ["Terminal sessions", info.data.terminalSessions],
                    ]}
                  />
                  <div className="flex items-center gap-2 text-[12px] text-muted">
                    <KeyRound className="size-3.5" /> TLS certificate verification is always enforced. <Badge tone="ok">secure</Badge>
                  </div>
                </>
              )}
            </div>
          </Section>
          <Field label="" hint="Manifex – native Kubernetes management. Licensed under Apache-2.0.">
            <span />
          </Field>
        </div>
      </div>
    </div>
  );
}
