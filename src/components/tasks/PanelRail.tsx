import type { LucideIcon } from "lucide-react";
import { ChevronsRight } from "lucide-react";
import { cn } from "@/lib/utils";

export interface PanelRailItem {
  key: string;
  icon: LucideIcon;
  count: number;
  /** Tooltip e nome accessibile (senza il numero). */
  label: string;
  /** Colore del contatore (classe `text-*`). */
  className: string;
  /** Animazione dell'icona (es. spinner dei task in corso). */
  iconClassName?: string;
}

interface PanelRailProps {
  ariaLabel: string;
  items: PanelRailItem[];
  onExpand: () => void;
}

/** Barra compressa di 48 px: pulsante di espansione e contatori con icona e colore. */
export default function PanelRail({ ariaLabel, items, onExpand }: PanelRailProps) {
  return (
    <aside
      aria-label={ariaLabel}
      className="flex w-12 flex-shrink-0 flex-col items-center gap-3.5 overflow-hidden rounded-[10px] border bg-base py-2"
    >
      <button
        type="button"
        onClick={onExpand}
        title="Espandi il pannello Task"
        aria-label="Espandi il pannello Task"
        className="flex h-8 w-8 items-center justify-center rounded-md text-text-secondary transition-colors hover:bg-surface-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <ChevronsRight className="h-3.5 w-3.5" aria-hidden="true" />
      </button>
      {items.map(({ key, icon: Icon, count, label, className, iconClassName }) => (
        <button
          key={key}
          type="button"
          onClick={onExpand}
          title={`${count} ${label}`}
          aria-label={`${count} ${label}: espandi il pannello Task`}
          className={cn(
            "flex w-full flex-col items-center gap-0.5 py-0.5 font-mono text-[11px] hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            className,
          )}
        >
          <Icon className={cn("h-3.5 w-3.5", iconClassName)} aria-hidden="true" />
          {count}
        </button>
      ))}
    </aside>
  );
}
