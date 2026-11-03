// Core UI primitives. Deliberately small and consistent: one button system, one
// input style, one badge style.

import * as RD from "@radix-ui/react-dialog";
import * as RM from "@radix-ui/react-dropdown-menu";
import * as RT from "@radix-ui/react-tooltip";
import { Loader2, X } from "lucide-react";
import React, { forwardRef } from "react";
import { cn } from "@/lib/format";
import type { Health } from "@/lib/types";

// ------------------------------------------------------------------ Button

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "subtle";
type ButtonSize = "sm" | "md" | "icon" | "icon-sm";

const variants: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover border border-transparent shadow-sm",
  secondary: "bg-panel text-fg border border-border hover:bg-hover hover:border-border-strong",
  ghost: "text-muted hover:text-fg hover:bg-hover border border-transparent",
  subtle: "bg-hover text-fg hover:bg-active border border-transparent",
  danger: "bg-err text-white hover:opacity-90 border border-transparent",
};

const sizes: Record<ButtonSize, string> = {
  sm: "h-7 px-2.5 text-[12px] gap-1.5 rounded-md",
  md: "h-8 px-3 text-[13px] gap-2 rounded-md",
  icon: "h-8 w-8 justify-center rounded-md",
  "icon-sm": "h-6 w-6 justify-center rounded",
};

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  icon?: React.ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", loading, icon, className, children, disabled, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      disabled={disabled || loading}
      className={cn(
        "inline-flex items-center font-medium whitespace-nowrap transition-colors duration-100 select-none",
        "disabled:opacity-50 disabled:pointer-events-none",
        variants[variant],
        sizes[size],
        className,
      )}
      {...rest}
    >
      {loading ? <Loader2 className="size-3.5 animate-spin" /> : icon}
      {children}
    </button>
  );
});

// ------------------------------------------------------------------ Inputs

export const Input = forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement> & { icon?: React.ReactNode }>(
  function Input({ className, icon, ...rest }, ref) {
    return (
      <div className={cn("relative flex items-center", className)}>
        {icon && <span className="pointer-events-none absolute left-2.5 text-subtle">{icon}</span>}
        <input
          ref={ref}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
          className={cn(
            "h-8 w-full rounded-md border border-border bg-panel px-2.5 text-[13px] text-fg placeholder:text-subtle",
            "transition-colors focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20",
            icon && "pl-8",
          )}
          {...rest}
        />
      </div>
    );
  },
);

export const Textarea = forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(function Textarea(
  { className, ...rest },
  ref,
) {
  return (
    <textarea
      ref={ref}
      spellCheck={false}
      className={cn(
        "w-full rounded-md border border-border bg-panel px-2.5 py-2 font-mono text-[12px] text-fg placeholder:text-subtle",
        "focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20",
        className,
      )}
      {...rest}
    />
  );
});

export function Select({
  value,
  onChange,
  options,
  className,
  ...rest
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
  className?: string;
} & Omit<React.SelectHTMLAttributes<HTMLSelectElement>, "onChange" | "value">) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={cn(
        "h-8 rounded-md border border-border bg-panel px-2 text-[13px] text-fg focus:border-accent focus:outline-none",
        className,
      )}
      {...rest}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Toggle({ checked, onChange, label, description, disabled }: { checked: boolean; onChange: (v: boolean) => void; label?: React.ReactNode; description?: React.ReactNode; disabled?: boolean }) {
  return (
    <label className={cn("flex items-start gap-3", disabled ? "opacity-50" : "cursor-pointer")}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cn(
          "relative mt-0.5 inline-flex h-[18px] w-8 shrink-0 rounded-full transition-colors",
          checked ? "bg-accent" : "bg-border-strong",
        )}
      >
        <span
          className={cn(
            "absolute top-[2px] size-[14px] rounded-full bg-white shadow transition-transform",
            checked ? "translate-x-[16px]" : "translate-x-[2px]",
          )}
        />
      </button>
      {(label || description) && (
        <span className="flex flex-col">
          {label && <span className="text-[13px] text-fg">{label}</span>}
          {description && <span className="text-[12px] text-muted">{description}</span>}
        </span>
      )}
    </label>
  );
}

export function Field({ label, hint, children, error }: { label: string; hint?: React.ReactNode; children: React.ReactNode; error?: string | null }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[12px] font-medium text-muted">{label}</span>
      {children}
      {error ? <span className="text-[12px] text-err">{error}</span> : hint ? <span className="text-[12px] text-subtle">{hint}</span> : null}
    </label>
  );
}

// ------------------------------------------------------------------ Status

const healthColor: Record<Health, string> = {
  ok: "bg-ok",
  warning: "bg-warn",
  error: "bg-err",
  pending: "bg-pending",
  neutral: "bg-neutral",
};

const healthText: Record<Health, string> = {
  ok: "text-ok",
  warning: "text-warn",
  error: "text-err",
  pending: "text-pending",
  neutral: "text-muted",
};

const healthSoft: Record<Health, string> = {
  ok: "bg-ok-soft text-ok",
  warning: "bg-warn-soft text-warn",
  error: "bg-err-soft text-err",
  pending: "bg-pending-soft text-pending",
  neutral: "bg-hover text-muted",
};

export function StatusDot({ health, pulse, className }: { health: Health; pulse?: boolean; className?: string }) {
  return (
    <span className={cn("relative inline-flex size-2 shrink-0", className)}>
      {pulse && <span className={cn("absolute inset-0 animate-ping rounded-full opacity-60", healthColor[health])} />}
      <span className={cn("relative inline-flex size-2 rounded-full", healthColor[health])} />
    </span>
  );
}

export function StatusText({ health, children }: { health: Health; children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center gap-2">
      <StatusDot health={health} />
      <span className={cn("truncate", health === "neutral" ? "text-muted" : healthText[health])}>{children}</span>
    </span>
  );
}

export function Badge({ children, tone = "neutral", className }: { children: React.ReactNode; tone?: Health | "accent"; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1 rounded px-1.5 text-[11px] font-medium whitespace-nowrap",
        tone === "accent" ? "bg-accent-soft text-accent" : healthSoft[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-panel px-1 font-sans text-[11px] text-muted">
      {children}
    </kbd>
  );
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn("size-4 animate-spin text-subtle", className)} />;
}

export function EmptyState({ icon, title, description, action }: { icon?: React.ReactNode; title: string; description?: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div className="flex h-full min-h-48 flex-col items-center justify-center gap-3 px-6 text-center">
      {icon && <div className="flex size-10 items-center justify-center rounded-lg bg-hover text-muted">{icon}</div>}
      <div className="flex flex-col gap-1">
        <div className="text-[14px] font-medium text-fg">{title}</div>
        {description && <div className="max-w-md text-[13px] text-muted">{description}</div>}
      </div>
      {action}
    </div>
  );
}

export function ErrorBanner({ message, className }: { message: string; className?: string }) {
  return (
    <div className={cn("selectable rounded-md border border-err/30 bg-err-soft px-3 py-2 text-[12px] text-err", className)}>{message}</div>
  );
}

export function Meter({ value, tone = "accent", className }: { value: number | null; tone?: "accent" | Health; className?: string }) {
  const v = value ?? 0;
  const color =
    tone === "accent" ? (v > 90 ? "bg-err" : v > 75 ? "bg-warn" : "bg-accent") : healthColor[tone as Health];
  return (
    <div className={cn("h-1.5 w-full overflow-hidden rounded-full bg-hover", className)}>
      <div className={cn("h-full rounded-full transition-[width] duration-500", color)} style={{ width: `${Math.max(0, Math.min(100, v))}%` }} />
    </div>
  );
}

// ------------------------------------------------------------------ Dialog

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  width = 520,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  children?: React.ReactNode;
  footer?: React.ReactNode;
  width?: number;
}) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className="fixed inset-0 z-40 animate-fade-in bg-black/40 backdrop-blur-[1px]" />
        <RD.Content
          style={{ width: `min(${width}px, calc(100vw - 48px))` }}
          className="fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100vh-64px)] -translate-x-1/2 -translate-y-1/2 animate-pop-in flex-col rounded-xl border border-border bg-elevated shadow-pop focus:outline-none"
        >
          <div className="flex items-start justify-between gap-4 border-b border-border px-5 pt-4 pb-3">
            <div className="flex flex-col gap-0.5">
              <RD.Title className="text-[15px] font-semibold text-fg">{title}</RD.Title>
              {description ? (
                <RD.Description className="text-[12.5px] text-muted">{description}</RD.Description>
              ) : (
                <RD.Description className="sr-only">{typeof title === "string" ? title : "Dialog"}</RD.Description>
              )}
            </div>
            <RD.Close asChild>
              <Button variant="ghost" size="icon-sm" aria-label="Close">
                <X className="size-4" />
              </Button>
            </RD.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-auto px-5 py-4">{children}</div>
          {footer && <div className="flex items-center justify-end gap-2 border-t border-border px-5 py-3">{footer}</div>}
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}

// ------------------------------------------------------------------ Menu

export const Menu = RM.Root;
export const MenuTrigger = RM.Trigger;

export function MenuContent({ children, align = "end", className }: { children: React.ReactNode; align?: "start" | "end" | "center"; className?: string }) {
  return (
    <RM.Portal>
      <RM.Content
        align={align}
        sideOffset={4}
        className={cn("z-50 min-w-48 animate-pop-in rounded-lg border border-border bg-elevated p-1 shadow-pop", className)}
      >
        {children}
      </RM.Content>
    </RM.Portal>
  );
}

export function MenuItem({
  children,
  onSelect,
  icon,
  danger,
  disabled,
  shortcut,
}: {
  children: React.ReactNode;
  onSelect?: () => void;
  icon?: React.ReactNode;
  danger?: boolean;
  disabled?: boolean;
  shortcut?: string;
}) {
  return (
    <RM.Item
      disabled={disabled}
      onSelect={onSelect}
      className={cn(
        "flex h-8 cursor-default items-center gap-2 rounded-md px-2 text-[13px] outline-none",
        "data-[disabled]:pointer-events-none data-[disabled]:opacity-40",
        danger ? "text-err data-[highlighted]:bg-err-soft" : "text-fg data-[highlighted]:bg-hover",
      )}
    >
      <span className="flex size-4 items-center justify-center text-muted">{icon}</span>
      <span className="flex-1">{children}</span>
      {shortcut && <span className="text-[11px] text-subtle">{shortcut}</span>}
    </RM.Item>
  );
}

export function MenuSeparator() {
  return <RM.Separator className="my-1 h-px bg-border" />;
}

export function MenuLabel({ children }: { children: React.ReactNode }) {
  return <RM.Label className="px-2 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase">{children}</RM.Label>;
}

// ------------------------------------------------------------------ Tooltip

export function Tooltip({ content, children, side = "top" }: { content: React.ReactNode; children: React.ReactElement; side?: "top" | "bottom" | "left" | "right" }) {
  if (!content) return children;
  return (
    <RT.Root delayDuration={350}>
      <RT.Trigger asChild>{children}</RT.Trigger>
      <RT.Portal>
        <RT.Content side={side} sideOffset={6} className="z-50 animate-fade-in rounded-md bg-fg px-2 py-1 text-[12px] text-canvas shadow-pop">
          {content}
        </RT.Content>
      </RT.Portal>
    </RT.Root>
  );
}

export const TooltipProvider = RT.Provider;

// ------------------------------------------------------------------ Tabs

export function Tabs<T extends string>({
  value,
  onChange,
  tabs,
  className,
}: {
  value: T;
  onChange: (v: T) => void;
  tabs: { id: T; label: React.ReactNode; count?: number; hidden?: boolean }[];
  className?: string;
}) {
  return (
    <div className={cn("flex items-center gap-1 overflow-x-auto", className)} role="tablist">
      {tabs
        .filter((t) => !t.hidden)
        .map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={value === t.id}
            onClick={() => onChange(t.id)}
            className={cn(
              "relative flex h-8 items-center gap-1.5 rounded-md px-2.5 text-[13px] whitespace-nowrap transition-colors",
              value === t.id ? "bg-hover font-medium text-fg" : "text-muted hover:bg-hover/60 hover:text-fg",
            )}
          >
            {t.label}
            {t.count !== undefined && <span className="tabular text-[11px] text-subtle">{t.count}</span>}
          </button>
        ))}
    </div>
  );
}

// ------------------------------------------------------------------ Section / Card

export function Section({ title, actions, children, className }: { title?: React.ReactNode; actions?: React.ReactNode; children: React.ReactNode; className?: string }) {
  return (
    <section className={cn("rounded-lg border border-border bg-panel", className)}>
      {(title || actions) && (
        <header className="flex h-10 items-center justify-between gap-3 border-b border-border px-4">
          <h3 className="text-[13px] font-semibold text-fg">{title}</h3>
          <div className="flex items-center gap-1.5">{actions}</div>
        </header>
      )}
      {children}
    </section>
  );
}

export function KeyValue({ items }: { items: [React.ReactNode, React.ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[minmax(110px,max-content)_1fr] gap-x-4 gap-y-1.5 text-[12.5px]">
      {items.map(([k, v], i) => (
        <React.Fragment key={i}>
          <dt className="text-muted">{k}</dt>
          <dd className="selectable min-w-0 break-words text-fg">{v ?? "—"}</dd>
        </React.Fragment>
      ))}
    </dl>
  );
}
