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
  /** Filtro agente: solo con almeno due agenti fra cui scegliere (`agentChoices` vuoto = nascosto). */
  agentChoices: FilterChoice[];
  agents: string[];
  onAgents: (agents: string[]) => void;
  filtering: boolean;
  onReset: () => void;
}

/** Una voce di un filtro a chip: il valore filtrato e il testo mostrato. */
export interface FilterChoice {
  value: string;
  label: string;
}

const chipClass =
  "flex h-[30px] items-center gap-1.5 rounded-full border px-2.5 text-[13px] transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

const chipTone = (on: boolean) =>
  on
    ? "border-primary bg-surface-2 text-foreground"
    : "border-border-strong bg-transparent text-muted-foreground hover:text-foreground";

interface ChoiceChipsProps {
  label: string;
  choices: FilterChoice[];
  selected: string[];
  onChange: (selected: string[]) => void;
  chipExtraClass?: string;
}

/** Chip a selezione multipla di un filtro (repository, agente). */
function ChoiceChips({ label, choices, selected, onChange, chipExtraClass }: ChoiceChipsProps) {
  return (
    <div role="group" aria-label={label} className="flex flex-wrap items-center gap-2">
      {choices.map((c) => {
        const on = selected.includes(c.value);
        return (
          <button
            key={c.value}
            type="button"
            aria-pressed={on}
            onClick={() => onChange(on ? selected.filter((x) => x !== c.value) : [...selected, c.value])}
            className={cn(chipClass, chipExtraClass, chipTone(on))}
          >
            {c.label}
          </button>
        );
      })}
    </div>
  );
}

/** Chip di stato a selezione multipla (con conteggi), filtri repo e agente e "Azzera filtri". */
export default function ListFilters({
  statuses,
  counts,
  onStatuses,
  repoChoices,
  repos,
  onRepos,
  agentChoices,
  agents,
  onAgents,
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
              className={cn(chipClass, chipTone(on))}
            >
              <span className={cn("h-[7px] w-[7px] rounded-full", statusDotClass(s))} aria-hidden="true" />
              {statusLabel(s)}
              <span className="font-mono text-[11px] text-muted-foreground">{counts[s]}</span>
            </button>
          );
        })}
      </div>
      {repoChoices.length > 1 && (
        <ChoiceChips
          label="Filtra per repository"
          choices={repoChoices.map((r) => ({ value: r, label: r }))}
          selected={repos}
          onChange={onRepos}
          chipExtraClass="font-mono text-xs"
        />
      )}
      {agentChoices.length > 0 && (
        <ChoiceChips label="Filtra per agente" choices={agentChoices} selected={agents} onChange={onAgents} />
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
