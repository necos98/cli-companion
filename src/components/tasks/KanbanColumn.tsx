import type { DragEvent, ReactNode } from "react";
import { cn } from "../../lib/utils";

interface Props {
  label: string;
  /** Classe del pallino (colore dello stato). */
  dotClass: string;
  count: number;
  hint?: string;
  /** Un trascinamento è in corso e questa colonna accetta il drop: evidenziata. */
  accepting: boolean;
  /** Un trascinamento è in corso e questa colonna non c'entra: attenuata. */
  dimmed: boolean;
  onDragOver?: (e: DragEvent<HTMLElement>) => void;
  onDrop?: (e: DragEvent<HTMLElement>) => void;
  /** Contenuto in fondo alla lista (link, area di rilascio). */
  footer?: ReactNode;
  children: ReactNode;
}

/** Colonna del Kanban: intestazione con conteggio e suggerimento, lista scorrevole. */
export default function KanbanColumn({
  label,
  dotClass,
  count,
  hint,
  accepting,
  dimmed,
  onDragOver,
  onDrop,
  footer,
  children,
}: Props) {
  return (
    <section
      aria-label={label}
      onDragOver={onDragOver}
      onDrop={onDrop}
      className={cn(
        "flex min-w-[240px] flex-[1_1_0] flex-col gap-2 rounded-[10px] border bg-base p-2.5 transition-[opacity,background-color,border-color]",
        accepting && "border-primary/50 bg-primary/5",
        dimmed && "opacity-45",
      )}
    >
      <div className="flex items-center gap-2 px-1 pb-1.5 pt-0.5">
        <span className={cn("h-2 w-2 flex-shrink-0 rounded-full", dotClass)} aria-hidden="true" />
        <h2 className="text-[13px] font-semibold">{label}</h2>
        <span className="font-mono text-[11px] text-muted-foreground">{count}</span>
        {hint && <span className="ml-auto truncate text-[11px] text-muted-foreground">{hint}</span>}
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-1">
        {children}
        {footer}
      </div>
    </section>
  );
}
