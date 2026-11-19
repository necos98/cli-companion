import DetailSection from "./DetailSection";
import { useCommand } from "./useDetailCommand";
import { Button } from "../ui/button";
import { useStore } from "../../hooks/useStore";
import { configStore } from "../../stores/config";
import { displayStatus, statusDotClass, statusLabel } from "../../lib/board";
import type { BoardView } from "../../lib/board";
import { deliverBotChoices, deliverBotId } from "../../lib/goalDelivery";
import { deliveryAction, deliveryTask } from "../../lib/goals";
import type { GoalView } from "../../lib/goals";
import { api } from "../../lib/tauri";
import { selectClass } from "../settings/shared";
import { cn } from "../../lib/utils";

interface Props {
  workspaceId: string;
  goal: GoalView;
  board: BoardView;
  onBoard: (board: BoardView) => void;
  onSelectTask: (taskId: string) => void;
}

/** Consegna di un goal aperto: consegna automatica, Bot che la esegue, task di consegna e pulsante "Consegna". */
export default function GoalDelivery({ workspaceId, goal, board, onBoard, onSelectTask }: Props) {
  const bots = useStore(configStore, (s) => s.botSettings.agents);
  const { run, busy, error } = useCommand(onBoard);
  const current = deliverBotId(goal);
  const choices = deliverBotChoices(bots, current);
  const task = deliveryTask(board.tasks, goal.id);
  const action = deliveryAction(goal);

  return (
    <DetailSection title="Consegna">
      <label className="flex items-center gap-2 text-[13px]">
        <input
          type="checkbox"
          checked={goal.auto_deliver}
          disabled={busy}
          onChange={(e) => void run(() => api.goalSetDelivery(workspaceId, goal.id, e.target.checked, null))}
          className="h-3.5 w-3.5 accent-primary"
        />
        Consegna automatica
      </label>
      <p className="text-xs text-muted-foreground">
        Quando il goal è pronto e l’ultima review non ha trovato problemi, la consegna parte da sola.
      </p>
      <label className="flex flex-col gap-1 text-xs text-text-secondary">
        Bot che esegue la consegna
        <select
          className={selectClass}
          value={current}
          disabled={busy}
          onChange={(e) => void run(() => api.goalSetDelivery(workspaceId, goal.id, null, e.target.value))}
        >
          {choices.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </select>
      </label>
      {task && (
        <button
          type="button"
          onClick={() => onSelectTask(task.id)}
          className="flex w-full min-w-0 items-center gap-2 rounded-md border border-border-strong px-2.5 py-1.5 text-left text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span
            className={cn("h-1.5 w-1.5 flex-shrink-0 rounded-full", statusDotClass(displayStatus(task)))}
            aria-hidden="true"
          />
          <span className="min-w-0 flex-1 truncate">{task.title}</span>
          <span className="flex-shrink-0 text-muted-foreground">{statusLabel(displayStatus(task))}</span>
        </button>
      )}
      {action && (
        <Button
          type="button"
          size="sm"
          disabled={busy}
          onClick={() => void run(() => api.goalDeliver(workspaceId, goal.id))}
          className="self-start"
        >
          {action}
        </Button>
      )}
      {error && (
        <p role="alert" className="break-words text-xs text-destructive">
          {error}
        </p>
      )}
    </DetailSection>
  );
}
