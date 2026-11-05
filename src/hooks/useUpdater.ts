// App self-update state shared by the status bar badge and Settings.

import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { create } from "zustand";
import { errorMessage, isTauri } from "@/lib/ipc";

type Status = "idle" | "checking" | "available" | "none" | "downloading" | "ready" | "error";

interface UpdaterStore {
  status: Status;
  update: Update | null;
  progress: number | null;
  error: string | null;
  lastChecked: number | null;
  check: (silent?: boolean) => Promise<void>;
  install: () => Promise<void>;
}

export const useUpdater = create<UpdaterStore>((set, get) => ({
  status: "idle",
  update: null,
  progress: null,
  error: null,
  lastChecked: null,
  check: async (silent = false) => {
    if (!isTauri || get().status === "downloading") return;
    if (!silent) set({ status: "checking", error: null });
    try {
      const update = await check();
      set({ update, status: update ? "available" : "none", lastChecked: Date.now(), error: null });
    } catch (e) {
      // Silent background checks don't surface errors (offline, no published release yet).
      set(silent ? { lastChecked: Date.now() } : { status: "error", error: friendlyUpdateError(errorMessage(e)), lastChecked: Date.now() });
    }
  },
  install: async () => {
    const update = get().update;
    if (!update) return;
    let total = 0;
    let done = 0;
    set({ status: "downloading", progress: 0, error: null });
    try {
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        if (ev.event === "Progress") {
          done += ev.data.chunkLength;
          set({ progress: total ? Math.round((done / total) * 100) : null });
        }
        if (ev.event === "Finished") set({ status: "ready" });
      });
      set({ status: "ready" });
      await relaunch();
    } catch (e) {
      set({ status: "error", error: errorMessage(e) });
    }
  },
}));

/** Background check on start and every 6 hours (when enabled in settings). */
export function startUpdateChecks(enabled: boolean): () => void {
  if (!enabled) return () => {};
  const run = () => useUpdater.getState().check(true);
  const first = setTimeout(run, 5_000);
  const timer = setInterval(run, 6 * 60 * 60 * 1000);
  return () => {
    clearTimeout(first);
    clearInterval(timer);
  };
}

/** The updater plugin reports a missing latest.json (no published release yet) with a cryptic message. */
export function friendlyUpdateError(message: string): string {
  if (/valid release JSON|404|Not Found/i.test(message)) return "No published release found yet – you are on the newest available version.";
  return message;
}
