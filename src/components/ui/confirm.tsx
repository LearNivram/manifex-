import { useState } from "react";
import { create } from "zustand";
import { Button, Dialog, Input } from "./index";

interface ConfirmRequest {
  title: string;
  message?: string;
  confirmLabel?: string;
  danger?: boolean;
  /** User must type this exact text to confirm (production safety). */
  requireText?: string;
  resolve: (ok: boolean) => void;
}

const useConfirmStore = create<{ req: ConfirmRequest | null; set: (r: ConfirmRequest | null) => void }>((set) => ({
  req: null,
  set: (req) => set({ req }),
}));

/** Promise-based confirmation using the app's own dialog (no native alert boxes). */
export function confirm(opts: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
  return new Promise((resolve) => useConfirmStore.getState().set({ ...opts, resolve }));
}

export function ConfirmHost() {
  const { req, set } = useConfirmStore();
  if (!req) return null;
  return <ConfirmDialog key={req.title} req={req} set={set} />;
}

function ConfirmDialog({ req, set }: { req: ConfirmRequest; set: (r: ConfirmRequest | null) => void }) {
  const [typed, setTyped] = useState("");
  const done = (ok: boolean) => {
    req.resolve(ok);
    set(null);
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && done(false)}
      title={req.title}
      width={440}
      footer={
        <>
          <Button variant="ghost" onClick={() => done(false)}>
            Cancel
          </Button>
          <Button variant={req.danger ? "danger" : "primary"} onClick={() => done(true)} autoFocus={!req.requireText} disabled={!!req.requireText && typed !== req.requireText}>
            {req.confirmLabel ?? "Confirm"}
          </Button>
        </>
      }
    >
      {req.message && <p className="text-[13px] text-muted">{req.message}</p>}
      {req.requireText && (
        <div className="mt-3 flex flex-col gap-2">
          <span className="text-[12.5px]">
            Type <span className="font-mono font-semibold">{req.requireText}</span> to confirm.
          </span>
          <Input autoFocus value={typed} onChange={(e) => setTyped(e.target.value)} />
        </div>
      )}
    </Dialog>
  );
}
