import { STATUSES, statusDotClass, statusLabel, toggleStatus } from "../../lib/board";
import type { TaskStatus } from "../../lib/board";
import { cn } from "../../lib/utils";

interface Props {
  statuses: TaskStatus[];
  counts: Record<TaskStatus, number>;
  onStatuses: (statuses: TaskStatus[]) => void;
  /** Filtro repository: solo nei workspace multi-repo (`repoChoices` vuoto = nascosto). */
  repoChoices: string[];
  repos: string[];
  onRepos: (repos: string[]) => void;
  filtering: boolean;
  onReset: () => void;
}

const chipClass =
  "flex h-[30px] items-center gap-1.5 rounded-full border px-2.5 text-[13px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

/** Chip di stato a selezione multipla (con conteggi), filtro repo e "Azzera filtri". */
export default function ListFilters({
  statuses,
  counts,
  onStatuses,
  repoChoices,
  repos,
  onRepos,
  filtering,
  onReset,
}: Props) {
  return (
    <div className="flex flex-shrink-0 flex-wrap items-center gap-2 border-b px-4 py-3">
      <div role="group" aria-label="Filtra per stato" className="flex flex-wrap items-center gap-2">
        {STATUSES.map((s) => {
          const on = statuses.includes(s);
          return (
            <button
              key={s}
              type="button"
              aria-pressed={on}
              onClick={() => onStatuses(toggleStatus(statuses, s))}
              className={cn(
                chipClass,
                on
                  ? "border-primary bg-surface-2 text-foreground"
                  : "border-border-strong bg-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              <span className={cn("h-[7px] w-[7px] rounded-full", statusDotClass(s))} aria-hidden="true" />
              {statusLabel(s)}
              <span className="font-mono text-[11px] text-muted-foreground">{counts[s]}</span>
            </button>
          );
        })}
      </div>
      {repoChoices.length > 1 && (
        <div role="group" aria-label="Filtra per repository" className="flex flex-wrap items-center gap-2">
          {repoChoices.map((r) => {
            const on = repos.includes(r);
            return (
              <button
                key={r}
                type="button"
                aria-pressed={on}
                onClick={() => onRepos(on ? repos.filter((x) => x !== r) : [...repos, r])}
                className={cn(
                  chipClass,
                  "font-mono text-xs",
                  on
                    ? "border-primary bg-surface-2 text-foreground"
                    : "border-border-strong bg-transparent text-muted-foreground hover:text-foreground",
                )}
              >
                {r}
              </button>
            );
          })}
        </div>
      )}
      {filtering && (
        <button
          type="button"
          onClick={onReset}
          className="h-[30px] rounded px-2 text-[13px] text-primary hover:text-primary/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          Azzera filtri
        </button>
      )}
    </div>
  );
}
