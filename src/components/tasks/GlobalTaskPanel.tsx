import { useMemo } from "react";
import { AlertTriangle, ChevronsLeft, ListOrdered, Loader2, Sparkles, XCircle } from "lucide-react";
import type { BoardView } from "../../lib/board";
import type { Workspace } from "../../lib/tauri";
import {
  activityGroups,
  activityTotals,
  groupSummary,
  proposalsText,
} from "../../lib/taskPanel";
import EnvChip from "../EnvChip";
import PanelHeaderButton from "./PanelHeaderButton";
import PanelRail from "./PanelRail";
import type { PanelRailItem } from "./PanelRail";
import PanelRunningCard from "./PanelRunningCard";
import { usePanelNow } from "./usePanelNow";

/**
 * Props del pannello Task della vista Globale: raggruppa per workspace (solo
 * quelli con task attivi o proposte). Accetta/Scarta e Task Intake non ci sono:
 * si fanno dentro il workspace (`onOpenWorkspace`).
 */
export interface GlobalTaskPanelProps {
  /** Workspace visibili (non archiviati), nell'ordine della sidebar. */
  workspaces: Workspace[];
  /** Board note per id di workspace (può mancare chi non ha ancora risposto). */
  boards: Record<string, BoardView>;
  /** Passa alla vista del workspace (selezione come dalla sidebar). */
  onOpenWorkspace: (workspaceId: string) => void;
  /** Mette a fuoco un tab nella griglia (es. il worker di un task in corso). */
  onFocusTab: (tabId: string) => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
}

/** Pannello Task della vista Globale: un gruppo per workspace con attività. */
export default function GlobalTaskPanel({
  workspaces,
  boards,
  onOpenWorkspace,
  onFocusTab,
  collapsed,
  onToggleCollapsed,
}: GlobalTaskPanelProps) {
  const groups = useMemo(() => activityGroups(workspaces, boards), [workspaces, boards]);
  const totals = useMemo(() => activityTotals(groups), [groups]);
  const now = usePanelNow(!collapsed && totals.running > 0);

  if (collapsed) {
    const items: PanelRailItem[] = [
      { key: "proposals", icon: Sparkles, count: totals.proposals, label: "proposte in attesa", className: "text-primary" },
      {
        key: "running",
        icon: Loader2,
        count: totals.running - totals.attention,
        label: "task in corso",
        className: "text-status-running",
      },
      {
        key: "attention",
        icon: AlertTriangle,
        count: totals.attention,
        label: "task richiedono attenzione",
        className: "text-status-attention",
      },
      { key: "queued", icon: ListOrdered, count: totals.queued, label: "task in coda", className: "text-muted-foreground" },
      { key: "failed", icon: XCircle, count: totals.failed, label: "task falliti", className: "text-destructive" },
    ];
    return (
      <PanelRail
        ariaLabel="Task di tutti i workspace, compresso"
        items={items.filter((i) => i.count > 0 || i.key === "queued" || i.key === "running")}
        onExpand={onToggleCollapsed}
      />
    );
  }

  return (
    <aside
      aria-label="Task di tutti i workspace"
      className="flex min-h-0 w-[284px] flex-shrink-0 flex-col overflow-hidden rounded-[10px] border bg-base"
    >
      <header className="flex h-10 flex-shrink-0 items-center gap-2 border-b bg-pane-header pl-3 pr-2">
        <span className="min-w-0 flex-1 truncate text-[13px] font-semibold">Task · tutti i workspace</span>
        <span
          className="flex-shrink-0 font-mono text-[11px]"
          title={`${totals.running} in corso, ${totals.queued} in coda`}
          aria-label={`${totals.running} in corso, ${totals.queued} in coda`}
        >
          <span className="text-status-running">{totals.running}</span>
          <span className="text-muted-foreground"> · {totals.queued}</span>
        </span>
        <PanelHeaderButton icon={ChevronsLeft} label="Comprimi il pannello Task" onClick={onToggleCollapsed} />
      </header>

      <div className="flex min-h-0 flex-1 flex-col gap-[18px] overflow-y-auto overflow-x-hidden p-2.5">
        {groups.map((g) => {
          const ws = g.workspace;
          return (
            <section key={ws.id} aria-label={ws.name} className="flex min-w-0 flex-col gap-1.5">
              <button
                type="button"
                onClick={() => onOpenWorkspace(ws.id)}
                title={`Apri ${ws.name}`}
                className="flex min-w-0 items-center gap-2 rounded px-1 pb-0.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                <EnvChip ws={ws} />
                <span className="min-w-0 flex-1 truncate text-[13px] font-semibold">{ws.name}</span>
                <span className="flex-shrink-0 text-[11px] text-muted-foreground">{groupSummary(g)}</span>
              </button>

              {g.running.map((t) => (
                <PanelRunningCard
                  key={t.id}
                  task={t}
                  now={now}
                  onClick={() => (t.session ? onFocusTab(t.session) : onOpenWorkspace(ws.id))}
                />
              ))}

              {g.next && (
                <p className="truncate px-1.5 text-xs text-muted-foreground" title={g.next.title}>
                  Prossimo: {g.next.title}
                </p>
              )}

              {g.proposals > 0 && (
                <button
                  type="button"
                  onClick={() => onOpenWorkspace(ws.id)}
                  className="flex items-center gap-1.5 self-start rounded px-1.5 text-xs text-primary hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  <Sparkles className="h-3 w-3 flex-shrink-0" aria-hidden="true" />
                  {proposalsText(g.proposals)}
                </button>
              )}
            </section>
          );
        })}

        <p className="rounded-lg border border-dashed border-border-strong p-3 text-xs leading-normal text-muted-foreground">
          {groups.length === 0 ? "Nessun workspace ha task attivi. " : "I workspace senza task attivi non compaiono. "}
          Proposte e Task Intake si usano dentro il workspace.
        </p>
      </div>
    </aside>
  );
}
