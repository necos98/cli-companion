import { useState } from "react";
import { Loader2, PackageCheck } from "lucide-react";
import type { BoardView } from "../../lib/board";
import { GOAL_WAITING_LABELS } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";
import { report } from "../../lib/errors";
import { api } from "../../lib/tauri";

interface Props {
  workspaceId: string;
  goal: GoalView;
  /** Pulsante: "Consegna" o "Consegna comunque". */
  label: string;
  onBoard: (board: BoardView) => void;
}

/** Card di un goal pronto da consegnare, con il pulsante che crea il task di consegna. */
export default function ReadyGoalCard({ workspaceId, goal, label, onBoard }: Props) {
  const [pending, setPending] = useState(false);
  const deliver = async () => {
    setPending(true);
    try {
      onBoard(await api.goalDeliver(workspaceId, goal.id));
    } catch (e) {
      report("consegna del goal", e, { notify: true });
    } finally {
      setPending(false);
    }
  };
  return (
    <div className="flex min-w-0 items-start gap-2.5 rounded-lg border border-primary/45 px-2.5 py-[9px]">
      <PackageCheck className="mt-px h-[15px] w-[15px] flex-shrink-0 text-primary" aria-hidden="true" />
      <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="line-clamp-2 break-words text-[13px] font-medium">{goal.title}</span>
        <span className="text-[11px] text-muted-foreground">
          {GOAL_WAITING_LABELS.deliver}
          {goal.last_review === "issues" && " · la review ha trovato problemi"}
        </span>
      </div>
      <button
        type="button"
        disabled={pending}
        onClick={() => void deliver()}
        className="flex h-7 flex-shrink-0 items-center gap-1.5 rounded-md bg-primary px-2.5 text-xs font-semibold text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
      >
        {pending && <Loader2 className="h-3 w-3 animate-spin motion-reduce:animate-none" aria-hidden="true" />}
        {label}
      </button>
    </div>
  );
}
