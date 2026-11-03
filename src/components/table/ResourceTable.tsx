import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/format";
import type { Row } from "@/lib/types";
import type { Column } from "@/features/resources/catalog";
import { useApp } from "@/store/app";

export interface ResourceTableProps {
  rows: Row[];
  columns: Column[];
  selectedUid?: string | null;
  onSelect?: (row: Row) => void;
  onActivate?: (row: Row) => void;
  onContextMenu?: (row: Row, e: React.MouseEvent) => void;
  rowActions?: (row: Row) => React.ReactNode;
  initialSort?: { id: string; desc?: boolean };
  empty?: React.ReactNode;
  checked?: Set<string>;
  onCheckedChange?: (s: Set<string>) => void;
}

/**
 * Virtualised, sortable table. Only visible rows are rendered, so tables with tens
 * of thousands of objects stay smooth.
 */
export function ResourceTable({
  rows,
  columns,
  selectedUid,
  onSelect,
  onActivate,
  onContextMenu,
  rowActions,
  initialSort,
  empty,
  checked,
  onCheckedChange,
}: ResourceTableProps) {
  const density = useApp((s) => s.settings?.density ?? "comfortable");
  const rowHeight = density === "compact" ? 28 : 34;
  const [sort, setSort] = useState<{ id: string; desc: boolean }>({ id: initialSort?.id ?? "name", desc: initialSort?.desc ?? false });
  const parentRef = useRef<HTMLDivElement>(null);

  const sorted = useMemo(() => {
    const col = columns.find((c) => c.id === sort.id);
    if (!col?.sort) return rows;
    const key = col.sort;
    const copy = rows.slice();
    copy.sort((a, b) => {
      const va = key(a);
      const vb = key(b);
      const r = typeof va === "number" && typeof vb === "number" ? va - vb : String(va).localeCompare(String(vb), undefined, { numeric: true });
      return r !== 0 ? (sort.desc ? -r : r) : a.name.localeCompare(b.name);
    });
    return copy;
  }, [rows, columns, sort]);

  const virtualizer = useVirtualizer({
    count: sorted.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => rowHeight,
    overscan: 12,
  });

  useEffect(() => virtualizer.measure(), [rowHeight, virtualizer]);

  const template = useMemo(() => {
    const cols = columns.map((c) => `minmax(${c.minWidth ?? 80}px, ${c.width ?? 1}fr)`);
    if (checked) cols.unshift("32px");
    if (rowActions) cols.push("40px");
    return cols.join(" ");
  }, [columns, rowActions, checked]);

  const selectedIndex = sorted.findIndex((r) => r.uid === selectedUid);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!sorted.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.max(0, Math.min(sorted.length - 1, (selectedIndex < 0 ? -1 : selectedIndex) + (e.key === "ArrowDown" ? 1 : -1)));
      onSelect?.(sorted[next]);
      virtualizer.scrollToIndex(next, { align: "auto" });
    } else if (e.key === "Enter" && selectedIndex >= 0) {
      onActivate?.(sorted[selectedIndex]);
    }
  };

  const allChecked = !!checked && sorted.length > 0 && sorted.every((r) => checked.has(r.uid));

  return (
    <div className="flex h-full min-h-0 flex-col" onKeyDown={onKeyDown} tabIndex={0} style={{ outline: "none" }}>
      <div
        className="grid shrink-0 items-center border-b border-border bg-panel px-3 text-[11.5px] font-medium text-muted"
        style={{ gridTemplateColumns: template, height: 32 }}
        role="row"
      >
        {checked && (
          <input
            type="checkbox"
            aria-label="Select all"
            className="accent-[var(--accent)]"
            checked={allChecked}
            onChange={() => onCheckedChange?.(allChecked ? new Set() : new Set(sorted.map((r) => r.uid)))}
          />
        )}
        {columns.map((c) => (
          <button
            key={c.id}
            disabled={!c.sort}
            onClick={() => setSort((s) => ({ id: c.id, desc: s.id === c.id ? !s.desc : false }))}
            className={cn("flex min-w-0 items-center gap-1 px-2 text-left hover:text-fg", c.align === "right" && "justify-end")}
          >
            <span className="truncate">{c.title}</span>
            {sort.id === c.id && (sort.desc ? <ArrowDown className="size-3" /> : <ArrowUp className="size-3" />)}
          </button>
        ))}
        {rowActions && <span />}
      </div>
      <div ref={parentRef} className="min-h-0 flex-1 overflow-auto" role="rowgroup">
        {sorted.length === 0 ? (
          empty
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualizer.getVirtualItems().map((vi) => {
              const row = sorted[vi.index];
              const selected = row.uid === selectedUid;
              return (
                <div
                  key={row.uid}
                  role="row"
                  aria-selected={selected}
                  onClick={() => onSelect?.(row)}
                  onDoubleClick={() => onActivate?.(row)}
                  onContextMenu={(e) => {
                    if (onContextMenu) {
                      e.preventDefault();
                      onContextMenu(row, e);
                    }
                  }}
                  className={cn(
                    "group absolute left-0 grid w-full items-center border-b border-border/60 px-3 text-[13px]",
                    selected ? "bg-accent-soft" : "hover:bg-hover",
                  )}
                  style={{ gridTemplateColumns: template, height: vi.size, transform: `translateY(${vi.start}px)` }}
                >
                  {checked && (
                    <input
                      type="checkbox"
                      aria-label={`Select ${row.name}`}
                      className="accent-[var(--accent)]"
                      checked={checked.has(row.uid)}
                      onClick={(e) => e.stopPropagation()}
                      onChange={() => {
                        const n = new Set(checked);
                        if (n.has(row.uid)) n.delete(row.uid);
                        else n.add(row.uid);
                        onCheckedChange?.(n);
                      }}
                    />
                  )}
                  {columns.map((c) => (
                    <div
                      key={c.id}
                      className={cn("flex min-w-0 items-center overflow-hidden px-2 whitespace-nowrap", c.align === "right" && "justify-end", c.mono && "font-mono text-[12px]")}
                    >
                      {c.render(row)}
                    </div>
                  ))}
                  {rowActions && (
                    <div className="flex justify-end opacity-0 group-hover:opacity-100 aria-selected:opacity-100" onClick={(e) => e.stopPropagation()}>
                      {rowActions(row)}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
