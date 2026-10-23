import { useState } from "react";
import type { FormEvent } from "react";
import DetailTaskFields from "./DetailTaskFields";
import { useCommand } from "./useDetailCommand";
import { Button } from "../ui/button";
import { api } from "../../lib/tauri";
import { buildPatch, formFromTask, hasErrors, validateTaskForm } from "../../lib/taskForm";
import type { TaskFormErrors, TaskFormValues } from "../../lib/taskForm";
import type { BoardView, Task } from "../../lib/board";

interface Props {
  workspaceId: string;
  task: Task;
  board: BoardView;
  onBoard: (board: BoardView) => void;
  /** Repository del workspace (più di uno = campo repo visibile). */
  repoChoices: string[];
  modelPlaceholder: string;
  /** Esce dalla modifica (salvato o annullato). */
  onDone: () => void;
}

/** Modifica di un task Da fare / Fallito: titolo, specifica, dipendenze, modello, repo. */
export default function TaskEditForm({
  workspaceId,
  task,
  board,
  onBoard,
  repoChoices,
  modelPlaceholder,
  onDone,
}: Props) {
  const [values, setValues] = useState<TaskFormValues>(() => formFromTask(task));
  const [errors, setErrors] = useState<TaskFormErrors>({});
  const { run, busy, error, clearError } = useCommand(onBoard);
  // Il repository si cambia solo senza worktree (come update_task nel backend).
  const repoLocked = Boolean(task.worktree);

  const change = (patch: Partial<TaskFormValues>) => {
    setValues((v) => ({ ...v, ...patch }));
    setErrors({});
    clearError();
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const found = validateTaskForm(values);
    setErrors(found);
    if (hasErrors(found)) return;
    const patch = buildPatch(task, values, repoChoices.length > 1 && !repoLocked);
    if (Object.keys(patch).length === 0) {
      onDone();
      return;
    }
    if (await run(() => api.taskUpdate(workspaceId, task.id, patch))) onDone();
  };

  return (
    <form onSubmit={submit} noValidate className="flex flex-col gap-4">
      <DetailTaskFields
        idPrefix={`edit-${task.id}`}
        values={values}
        onChange={change}
        errors={errors}
        tasks={board.tasks}
        selfId={task.id}
        repoChoices={repoChoices}
        repoLocked={repoLocked}
        modelPlaceholder={modelPlaceholder}
        disabled={busy}
        specRows={12}
      />
      {error && (
        <p role="alert" className="break-words text-xs text-destructive">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="submit" size="sm" disabled={busy} className="h-[30px] px-3 text-[13px]">
          {busy ? "Salvataggio…" : "Salva"}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={onDone}
          className="h-[30px] bg-transparent px-3 text-[13px]"
        >
          Annulla
        </Button>
      </div>
    </form>
  );
}
