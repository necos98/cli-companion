import { useState } from "react";
import type { FormEvent } from "react";
import DetailTaskFields from "./DetailTaskFields";
import { useCommand } from "./useDetailCommand";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../ui/dialog";
import { sanitizeModel } from "../../lib/model";
import { api } from "../../lib/tauri";
import {
  ROOT_REPO,
  findCreatedTask,
  hasErrors,
  isMultiRepo,
  repoOptions,
  repoValue,
  validateTaskForm,
} from "../../lib/taskForm";
import type { TaskFormErrors, TaskFormValues } from "../../lib/taskForm";
import type { BoardView, Task } from "../../lib/board";
import type { Workspace } from "../../lib/tauri";

/**
 * Props del dialog "Nuovo task": titolo, specifica, dipendenze (multi-selezione
 * tra i task non Fatti), modello (default del workspace), repo (solo
 * multi-repo; `api.workspaceDetectRepos`) e il suggerimento "Per una specifica
 * completa usa Task Intake". Crea con `api.taskCreate` e passa a `onBoard` la
 * board restituita, poi chiama `onClose`.
 */
export interface NewTaskDialogProps {
  open: boolean;
  workspace: Workspace;
  /** Board corrente (null = non ancora caricata): serve alle dipendenze possibili. */
  board: BoardView | null;
  onBoard: (board: BoardView) => void;
  onClose: () => void;
  /** Apre un nuovo tab Task Intake (link del suggerimento). */
  onOpenIntake: () => void;
  /** Repository rilevati del workspace (`workspace_detect_repos`); assente = solo quelli dei task. */
  repos?: string[];
  /** Task appena creato (null se non lo si ritrova nella board restituita): il manager lo seleziona. */
  onCreated?: (task: Task | null) => void;
}

const NO_REPOS: string[] = [];

/** Dialog di creazione di un task manuale (senza proposta). */
export default function NewTaskDialog({
  open,
  workspace,
  board,
  onBoard,
  onClose,
  onOpenIntake,
  repos = NO_REPOS,
  onCreated,
}: NewTaskDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {/* Il contenuto si smonta alla chiusura: il form riparte vuoto a ogni apertura. */}
      <DialogContent className="max-h-[90vh] max-w-xl overflow-y-auto">
        <NewTaskForm
          workspace={workspace}
          board={board}
          onBoard={onBoard}
          onClose={onClose}
          onOpenIntake={onOpenIntake}
          repos={repos}
          onCreated={onCreated}
        />
      </DialogContent>
    </Dialog>
  );
}

type FormProps = Omit<NewTaskDialogProps, "open"> & { repos: string[] };

function NewTaskForm({ workspace, board, onBoard, onClose, onOpenIntake, repos, onCreated }: FormProps) {
  const tasks = board?.tasks ?? [];
  const repoChoices = repoOptions(tasks, repos);
  const multiRepo = isMultiRepo(repoChoices);
  const [values, setValues] = useState<TaskFormValues>({
    title: "",
    task: "",
    depends_on: [],
    model: "",
    repo: repoChoices[0] ?? ROOT_REPO,
  });
  const [errors, setErrors] = useState<TaskFormErrors>({});
  const { run, busy, error, clearError } = useCommand(onBoard);
  // I repository possono arrivare dopo l'apertura: la scelta vale solo se è fra quelli noti.
  const repo = repoChoices.includes(values.repo) ? values.repo : (repoChoices[0] ?? ROOT_REPO);

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
    const previous = new Set(tasks.map((t) => t.id));
    let created: Task | null = null;
    const ok = await run(async () => {
      const next = await api.taskCreate(workspace.id, {
        title: values.title.trim(),
        task: values.task.trim(),
        depends_on: values.depends_on,
        repo: multiRepo ? repoValue(repo) : null,
        model: sanitizeModel(values.model),
      });
      created = findCreatedTask(next, values.title, previous);
      return next;
    });
    if (!ok) return;
    onCreated?.(created);
    onClose();
  };

  return (
    <form onSubmit={submit} noValidate className="flex flex-col gap-4">
      <DialogHeader>
        <DialogTitle>Nuovo task</DialogTitle>
        <DialogDescription>Entra in fondo alla coda di {workspace.name} e parte quando può.</DialogDescription>
      </DialogHeader>

      <div className="flex items-center justify-between gap-3 rounded-md border bg-surface px-3 py-2 text-xs">
        <span className="text-text-secondary">Per una specifica completa usa Task Intake.</span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            onClose();
            onOpenIntake();
          }}
          className="h-7 flex-shrink-0 bg-transparent px-2.5 text-xs"
        >
          Apri Task Intake
        </Button>
      </div>

      <DetailTaskFields
        idPrefix="new-task"
        values={{ ...values, repo }}
        onChange={change}
        errors={errors}
        tasks={tasks}
        selfId={null}
        repoChoices={repoChoices}
        modelPlaceholder={workspace.worker_model || "predefinito dei worker"}
        disabled={busy}
      />

      {error && (
        <p role="alert" className="break-words text-xs text-destructive">
          {error}
        </p>
      )}

      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onClose} className="bg-transparent">
          Annulla
        </Button>
        <Button type="submit" size="sm" disabled={busy}>
          {busy ? "Creazione…" : "Crea task"}
        </Button>
      </div>
    </form>
  );
}
