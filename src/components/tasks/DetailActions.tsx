import { useState } from "react";
import { ArrowUpToLine, CheckCheck, ExternalLink, Pencil, RotateCw, SquareTerminal, Trash2, OctagonX } from "lucide-react";
import { Button } from "../ui/button";
import {
  ACTION_LABELS,
  canDeleteTask,
  canEditTask,
  dependentsOf,
  prUrl,
  queuedTasks,
} from "../../lib/board";
import type { BoardView, Task, TaskAction } from "../../lib/board";

interface Props {
  task: Task;
  board: BoardView;
  busy: boolean;
  /** Il tab del worker si può aprire. */
  canOpenTerminal: boolean;
  onEdit: () => void;
  onAction: (action: TaskAction) => void;
  onOpenTerminal: () => void;
  onOpenPr: (url: string) => void;
}

type Confirm = "delete" | "stop" | "mark_done";

const CONFIRM_TEXT: Record<Confirm, string> = {
  delete: "Eliminare questo task? L'operazione non si può annullare.",
  stop: "Fermare il worker? Il task passa a Fallito e il terminale viene chiuso.",
  mark_done: "Segnare il task come fatto? Il lavoro non viene verificato né integrato dall'app.",
};

const btn = "h-[30px] gap-1.5 px-3 text-[13px]";
const ghost = `${btn} bg-transparent`;

/** Azioni del task per stato; Elimina, Ferma e Segna come fatto chiedono conferma in linea. */
export default function DetailActions({
  task,
  board,
  busy,
  canOpenTerminal,
  onEdit,
  onAction,
  onOpenTerminal,
  onOpenPr,
}: Props) {
  const [confirm, setConfirm] = useState<Confirm | null>(null);

  if (confirm) {
    return (
      <div
        role="alertdialog"
        aria-label="Conferma"
        className="flex flex-col gap-2 rounded-md border border-destructive/40 bg-destructive/5 p-2.5"
      >
        <p className="text-xs">{CONFIRM_TEXT[confirm]}</p>
        <div className="flex gap-2">
          <Button
            type="button"
            variant={confirm === "mark_done" ? "default" : "destructive"}
            disabled={busy}
            autoFocus
            onClick={() => {
              onAction(confirm);
              setConfirm(null);
            }}
            className={btn}
          >
            {ACTION_LABELS[confirm]}
          </Button>
          <Button type="button" variant="outline" onClick={() => setConfirm(null)} className={ghost}>
            Annulla
          </Button>
        </div>
      </div>
    );
  }

  const pr = prUrl(task);
  const canDelete = canDeleteTask(task, board);
  const blockers = dependentsOf(task.id, board);
  const isFirst = queuedTasks(board)[0]?.id === task.id;

  const terminal = canOpenTerminal && (
    <Button type="button" onClick={onOpenTerminal} className={btn}>
      <SquareTerminal /> Apri terminale
    </Button>
  );
  const edit = canEditTask(task) && (
    <Button type="button" variant="outline" disabled={busy} onClick={onEdit} className={ghost}>
      <Pencil /> Modifica
    </Button>
  );
  const remove = (task.status === "queued" || task.status === "failed") && (
    <Button
      type="button"
      variant="outline"
      disabled={busy || !canDelete}
      title={canDelete ? undefined : `Prima elimina o completa i task che ne dipendono: ${blockers.join(", ")}`}
      onClick={() => setConfirm("delete")}
      className={`${ghost} text-destructive hover:text-destructive`}
    >
      <Trash2 /> {ACTION_LABELS.delete}
    </Button>
  );

  return (
    <div className="flex flex-wrap gap-1.5">
      {task.status === "queued" && (
        <>
          {edit}
          <Button
            type="button"
            variant="outline"
            disabled={busy || isFirst}
            title={isFirst ? "È già il prossimo in coda" : undefined}
            onClick={() => onAction("to_top")}
            className={ghost}
          >
            <ArrowUpToLine /> {ACTION_LABELS.to_top}
          </Button>
          {remove}
        </>
      )}
      {task.status === "running" && (
        <>
          {terminal}
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => setConfirm("stop")}
            className={ghost}
          >
            <OctagonX /> {ACTION_LABELS.stop}
          </Button>
        </>
      )}
      {task.status === "done" && pr && (
        <Button type="button" onClick={() => onOpenPr(pr)} className={btn}>
          <ExternalLink /> Apri PR
        </Button>
      )}
      {task.status === "failed" && (
        <>
          <Button type="button" disabled={busy} onClick={() => onAction("retry")} className={btn}>
            <RotateCw /> {ACTION_LABELS.retry}
          </Button>
          <Button type="button" variant="outline" disabled={busy} onClick={() => setConfirm("mark_done")} className={ghost}>
            <CheckCheck /> {ACTION_LABELS.mark_done}
          </Button>
          {edit}
          {terminal && (
            <Button type="button" variant="outline" onClick={onOpenTerminal} className={ghost}>
              <SquareTerminal /> Apri terminale
            </Button>
          )}
          {remove}
        </>
      )}
    </div>
  );
}
