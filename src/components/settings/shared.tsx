import { useCallback, useState } from "react";
import type { ReactNode } from "react";
import type { Workspace } from "../../lib/tauri";
import { cn } from "../../lib/utils";

export const selectClass =
  "flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 [&>option]:bg-popover [&>option]:text-popover-foreground";

/** Pagina di impostazioni: titolo, descrizione di una riga, poi le sezioni. */
export function SettingsPage({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-sm font-semibold">{title}</h2>
        <p className="mt-0.5 text-[11px] text-muted-foreground">{description}</p>
      </div>
      {children}
    </div>
  );
}

/** Sezione a scheda di una pagina. */
export function Section({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={cn("flex flex-col gap-3 rounded-lg border bg-card p-3", className)}>
      {children}
    </section>
  );
}

/** Campo: etichetta, controllo, testo d'aiuto. */
export function Field({
  label,
  help,
  right,
  children,
}: {
  label: string;
  help?: ReactNode;
  /** Elemento a destra dell'etichetta (es. uno stato). */
  right?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between gap-2">
        <p className="text-xs font-semibold">{label}</p>
        {right}
      </div>
      {children}
      {help && <p className="mt-1 text-[11px] text-muted-foreground">{help}</p>}
    </div>
  );
}

/** Etichetta "integrato" degli avvii predefiniti dell'app. */
export function BuiltinBadge() {
  return (
    <span className="rounded border px-1 py-px text-[10px] font-normal text-muted-foreground">
      integrato
    </span>
  );
}

/**
 * Esegue una modifica al workspace tenendo traccia di stato di salvataggio ed
 * errore; il workspace aggiornato risale a chi lo possiede.
 */
export function useWorkspaceOp(onWorkspaceUpdated: (ws: Workspace) => void) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(
    async (op: () => Promise<Workspace>): Promise<boolean> => {
      setError(null);
      setSaving(true);
      try {
        onWorkspaceUpdated(await op());
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      } finally {
        setSaving(false);
      }
    },
    [onWorkspaceUpdated],
  );
  return { run, saving, error, setError };
}
