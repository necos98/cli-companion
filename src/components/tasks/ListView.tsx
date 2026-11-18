import { useMemo } from "react";
import { ArrowDown, ArrowUp } from "lucide-react";
import ListFilters from "./ListFilters";
import ListGroups from "./ListGroups";
import ListGroupToggle from "./ListGroupToggle";
import ListRow, { LIST_GRID } from "./ListRow";
import { isFiltering } from "../../lib/board";
import type { BoardView, Task } from "../../lib/board";
import { filterByGoal, goalLabel, groupByGoal } from "../../lib/goals";
import type { GoalFilter } from "../../lib/goals";
import { agentFilterOptions, agentName, showAgentFilter } from "../../lib/taskAgent";
import type { TaskAgents } from "../../lib/taskAgent";
import { listRows, listSubtitle, queuePositions, statusCounts, toggleSort } from "../../lib/taskViews";
import type { ListPrefs, ListSortKey } from "../../lib/taskViews";
import { cn } from "../../lib/utils";

interface Props {
  board: BoardView;
  /** Agenti del workspace: badge, motivo di un agente non pronto e nomi del filtro. */
  agents: TaskAgents;
  /** Ricerca della barra del manager. */
  query: string;
  onQuery: (query: string) => void;
  prefs: ListPrefs;
  onPrefs: (prefs: ListPrefs) => void;
  /** Filtro per goal (già ricondotto a un goal esistente) e sua modifica. */
  goalFilter: GoalFilter;
  onGoalFilter: (filter: GoalFilter) => void;
  /** Repository del workspace (più di uno = multi-repo). */
  repoChoices: string[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** Apre il dettaglio di un goal (intestazione di un gruppo). */
  onOpenGoal: (goalId: string) => void;
  now: number;
  onNewTask: () => void;
}

const HEADERS: { key: ListSortKey | null; label: string }[] = [
  { key: null, label: "Stato" },
  { key: "title", label: "Titolo" },
  { key: null, label: "Modello" },
  { key: "updated", label: "Aggiornato" },
  { key: "duration", label: "Durata" },
];

/**
 * Lista: chip di stato (conteggi che seguono la ricerca), filtro repo,
 * intestazioni ordinabili, righe-bottone, piede "N di M task".
 */
export default function ListView({
  board,
  agents,
  query,
  onQuery,
  prefs,
  onPrefs,
  goalFilter,
  onGoalFilter,
  repoChoices,
  selectedId,
  onSelect,
  onOpenGoal,
  now,
  onNewTask,
}: Props) {
  const tasks = useMemo(() => filterByGoal(board.tasks, goalFilter), [board.tasks, goalFilter]);
  const hasGoals = board.goals.length > 0;
  const grouped = hasGoals && prefs.groupByGoal;
  const multiRepo = repoChoices.length > 1;
  const agentIds = useMemo(() => agentFilterOptions(tasks, prefs.agents), [tasks, prefs.agents]);
  const agentFilter = showAgentFilter(agentIds);
  const agentChoices = useMemo(
    () => (agentFilter ? agentIds.map((id) => ({ value: id, label: agentName(agents, id) })) : []),
    [agentFilter, agentIds, agents],
  );
  const filter = useMemo(
    () => ({
      query,
      statuses: prefs.statuses,
      repos: multiRepo ? prefs.repos : [],
      agents: agentFilter ? prefs.agents : [],
    }),
    [query, prefs.statuses, prefs.repos, prefs.agents, multiRepo, agentFilter],
  );
  const rows = useMemo(() => listRows(tasks, filter, prefs.sort, now), [tasks, filter, prefs.sort, now]);
  const counts = useMemo(() => statusCounts(tasks, filter), [tasks, filter]);
  const positions = useMemo(() => queuePositions(board.tasks), [board.tasks]);
  const filtering = isFiltering(filter) || goalFilter !== null;

  const reset = () => {
    onQuery("");
    onGoalFilter(null);
    onPrefs({ ...prefs, statuses: [], repos: [], agents: [] });
  };

  const renderRow = (t: Task) => (
    <ListRow
      key={t.id}
      task={t}
      subtitle={listSubtitle(t, board.tasks, positions)}
      agents={agents}
      now={now}
      selected={t.id === selectedId}
      showRepo={multiRepo}
      goalTitle={grouped ? null : goalLabel(board.goals, t.goal)}
      onSelect={onSelect}
    />
  );

  return (
    <section aria-label="Elenco dei task" className="flex h-full min-h-0 min-w-0 flex-col">
      <ListFilters
        statuses={prefs.statuses}
        counts={counts}
        onStatuses={(statuses) => onPrefs({ ...prefs, statuses })}
        repoChoices={repoChoices}
        repos={prefs.repos}
        onRepos={(repos) => onPrefs({ ...prefs, repos })}
        agentChoices={agentChoices}
        agents={prefs.agents}
        onAgents={(selected) => onPrefs({ ...prefs, agents: selected })}
        filtering={filtering}
        onReset={reset}
        trailing={
          hasGoals && (
            <ListGroupToggle on={prefs.groupByGoal} onChange={(groupByGoal) => onPrefs({ ...prefs, groupByGoal })} />
          )
        }
      />

      <div className="min-h-0 flex-1 overflow-auto">
        <div
          className={cn(
            LIST_GRID,
            "sticky top-0 z-10 min-w-[700px] border-b bg-pane-area px-4 py-2 text-[11px] font-medium tracking-[0.06em] text-muted-foreground",
          )}
        >
          {HEADERS.map((h) => {
            const sortKey = h.key;
            if (!sortKey) return <span key={h.label}>{h.label.toUpperCase()}</span>;
            const active = prefs.sort.key === sortKey;
            const Arrow = prefs.sort.desc ? ArrowDown : ArrowUp;
            return (
              <button
                key={h.label}
                type="button"
                aria-label={`Ordina per ${h.label.toLowerCase()}`}
                aria-pressed={active}
                onClick={() => onPrefs({ ...prefs, sort: toggleSort(prefs.sort, sortKey) })}
                className={cn(
                  "flex items-center gap-1 text-left tracking-[0.06em] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                  active && "text-foreground",
                )}
              >
                {h.label.toUpperCase()}
                {active && <Arrow className="h-3 w-3" aria-hidden="true" />}
              </button>
            );
          })}
        </div>

        {grouped ? (
          <ListGroups groups={groupByGoal(rows, board.goals)} renderRow={renderRow} onOpenGoal={onOpenGoal} />
        ) : (
          rows.map(renderRow)
        )}

        {rows.length === 0 && (
          <div className="flex flex-col items-center gap-2 px-4 py-12 text-center text-[13px] text-muted-foreground">
            {board.tasks.length === 0 ? (
              <>
                <span>Nessun task in questo workspace.</span>
                <button
                  type="button"
                  onClick={onNewTask}
                  className="h-[30px] rounded-md border border-border-strong px-3 text-foreground hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  Nuovo task
                </button>
              </>
            ) : (
              <>
                <span>Nessun task corrisponde ai filtri.</span>
                <button
                  type="button"
                  onClick={reset}
                  className="h-[30px] rounded-md border border-border-strong px-3 text-foreground hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  Azzera filtri
                </button>
              </>
            )}
          </div>
        )}
      </div>

      <div className="flex-shrink-0 border-t px-4 py-2 text-xs text-muted-foreground">
        {rows.length} di {board.tasks.length} task
      </div>
    </section>
  );
}
