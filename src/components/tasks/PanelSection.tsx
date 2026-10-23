import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

interface PanelSectionProps {
  title: string;
  /** Contatore a destra ("2/3", "4"). */
  count?: string | number;
  /** Colore del titolo (di default grigio attenuato). */
  titleClassName?: string;
  children: ReactNode;
}

/** Sezione del corpo del pannello: titolo in maiuscolo con contatore, poi le card. */
export default function PanelSection({ title, count, titleClassName, children }: PanelSectionProps) {
  return (
    <section className="flex min-w-0 flex-col gap-1">
      <div className="flex items-center justify-between px-1 pb-1">
        <h3 className={cn("text-[11px] font-medium tracking-[0.08em] text-muted-foreground", titleClassName)}>
          {title}
        </h3>
        {count !== undefined && <span className="font-mono text-[11px] text-muted-foreground">{count}</span>}
      </div>
      {children}
    </section>
  );
}
