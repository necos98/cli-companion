import { Loader2 } from "lucide-react";
import { dismissFollowUp, promoteFollowUp } from "../../app/actions/followUps";
import type { BoardView } from "../../lib/board";
import { followUpStateLabel } from "../../lib/followUps";
import type { FollowUpEntry } from "../../lib/followUps";
import type { GoalView } from "../../lib/goals";
import { useCommand } from "./useDetailCommand";

interface Props {
  workspaceId: string;
  goal: GoalView;
  entry: FollowUpEntry;
  /** Mostra lo stato della voce (nel dettaglio del goal; in "Serve a te" sono tutte aperte). */
  showState?: boolean;
  onBoard: (board: BoardView) => void;
}

const ACTION_BUTTON =
  "flex h-[26px] items-center gap-1 rounded-md border border-border-strong px-2.5 text-xs transition-colors hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50";

/** Un follow-up: testo, task d'origine e, se aperto, "Scarta" e "Manda all'Intake". */
export default function FollowUpRow({ workspaceId, goal, entry, showState = false, onBoard }: Props) {
  const { followUp, index } = entry;
  const { run, busy, error } = useCommand(onBoard);
  const isOpen = followUp.state === "open";
  return (
    <div className="flex min-w-0 flex-col gap-1.5 rounded-md border bg-surface px-3 py-2">
      <p className="whitespace-pre-line break-words text-[13px] leading-relaxed">{followUp.text}</p>
      <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <span className="min-w-0 truncate font-mono" title={followUp.source_task}>
          {followUp.source_task}
        </span>
        {showState && (
          <>
            <span aria-hidden="true">·</span>
            <span>{followUpStateLabel(followUp.state)}</span>
          </>
        )}
      </span>
      {isOpen && (
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(() => promoteFollowUp(workspaceId, goal, index, followUp))}
            className={`${ACTION_BUTTON} bg-primary text-primary-foreground hover:bg-primary/90`}
          >
            {busy && <Loader2 className="h-3 w-3 animate-spin motion-reduce:animate-none" aria-hidden="true" />}
            Manda all'Intake
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(() => dismissFollowUp(workspaceId, goal.id, index))}
            className={ACTION_BUTTON}
          >
            Scarta
          </button>
        </div>
      )}
      {error && (
        <p role="alert" className="break-words text-xs text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
