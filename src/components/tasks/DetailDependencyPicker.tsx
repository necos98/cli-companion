import { useMemo, useState } from "react";
import { cn } from "../../lib/utils";
import { statusDotClass, statusLabel } from "../../lib/board";
import type { Task } from "../../lib/board";

interface Props {
  idPrefix: string;
  /** Task selezionabili (già senza cicli e senza i Fatti non selezionati). */
  options: Task[];
  selected: string[];
  onChange: (selected: string[]) => void;
  disabled?: boolean;
}

/** Multi-selezione delle dipendenze: elenco con caselle e, se lungo, un filtro. */
export default function DetailDependencyPicker({ idPrefix, options, selected, onChange, disabled }: Props) {
  const [filter, setFilter] = useState("");
  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return q === ""
      ? options
      : options.filter((t) => t.title.toLowerCase().includes(q) || t.id.toLowerCase().includes(q));
  }, [options, filter]);

  if (options.length === 0) {
    return <p className="text-xs text-muted-foreground">Nessun task da cui dipendere.</p>;
  }

  const toggle = (id: string) =>
    onChange(selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]);

  return (
    <div className="flex flex-col gap-1.5">
      {options.length > 8 && (
        <input
          type="search"
          aria-label="Filtra le dipendenze"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filtra…"
          className="h-8 w-full rounded-md border border-input bg-transparent px-2.5 text-xs outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring"
        />
      )}
      <ul
        aria-label="Dipendenze"
        className="max-h-40 overflow-y-auto rounded-md border border-input"
      >
        {shown.map((t) => {
          const id = `${idPrefix}-dep-${t.id}`;
          return (
            <li key={t.id}>
              <label
                htmlFor={id}
                className={cn(
                  "flex cursor-pointer items-center gap-2 px-2.5 py-1.5 text-xs hover:bg-surface-2",
                  disabled && "cursor-not-allowed opacity-60",
                )}
              >
                <input
                  id={id}
                  type="checkbox"
                  checked={selected.includes(t.id)}
                  disabled={disabled}
                  onChange={() => toggle(t.id)}
                  className="h-3.5 w-3.5 flex-shrink-0 accent-primary"
                />
                <span className="min-w-0 flex-1 truncate">{t.title}</span>
                <span className="flex-shrink-0 font-mono text-[11px] text-faint">{t.id}</span>
                <span className={cn("h-1.5 w-1.5 flex-shrink-0 rounded-full", statusDotClass(t.status))} aria-hidden="true" />
                <span className="sr-only">{statusLabel(t.status)}</span>
              </label>
            </li>
          );
        })}
        {shown.length === 0 && <li className="px-2.5 py-2 text-xs text-muted-foreground">Nessun risultato.</li>}
      </ul>
      {selected.length > 0 && (
        <p className="text-[11px] text-muted-foreground">
          {selected.length === 1 ? "1 dipendenza" : `${selected.length} dipendenze`}: il task parte quando sono tutte Fatte.
        </p>
      )}
    </div>
  );
}
