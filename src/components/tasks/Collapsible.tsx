import type { ReactNode } from "react";

/** Dettaglio a scomparsa con una riga di sommario. */
export default function Collapsible({ summary, children }: { summary: string; children: ReactNode }) {
  return (
    <details className="group text-xs">
      <summary className="cursor-pointer select-none rounded text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring">
        {summary}
      </summary>
      <div className="mt-1.5">{children}</div>
    </details>
  );
}
