// Form di creazione / modifica di un task (Tasks Manager): validazione lato
// client (il backend rivalida), dipendenze selezionabili senza cicli, patch dei
// soli campi cambiati, repository. Funzioni pure a zero dipendenze (solo
// board.ts e model.ts), importabili sia dall'app sia dai test `node --test`.

import { modelError, sanitizeModel } from "./model.ts";
import type { BoardView, Task, TaskPatch } from "./board.ts";

export const TITLE_MAX = 80;
export const SPEC_MAX = 100_000;
/** La root del workspace come valore di repository nel form. */
export const ROOT_REPO = ".";

export interface TaskFormValues {
  title: string;
  task: string;
  depends_on: string[];
  model: string;
  /** Repository scelto; `ROOT_REPO` = la root (si invia come null). */
  repo: string;
}

export type TaskFormErrors = Partial<Record<"title" | "task" | "model", string>>;

/** Errori di campo del form (vuoto = valido). */
export function validateTaskForm(v: Pick<TaskFormValues, "title" | "task" | "model">): TaskFormErrors {
  const errors: TaskFormErrors = {};
  const title = v.title.trim();
  if (title === "") errors.title = "Il titolo è obbligatorio";
  else if ([...title].length > TITLE_MAX) errors.title = `Il titolo può avere al massimo ${TITLE_MAX} caratteri`;
  const spec = v.task.trim();
  if (spec === "") errors.task = "La specifica è obbligatoria";
  else if ([...spec].length > SPEC_MAX) errors.task = `La specifica può avere al massimo ${SPEC_MAX} caratteri`;
  const model = modelError(v.model);
  if (model) errors.model = model;
  return errors;
}

export function hasErrors(e: TaskFormErrors): boolean {
  return Object.keys(e).length > 0;
}

/**
 * Task che possono essere dipendenza di `selfId` (null = task nuovo): i non
 * Fatti, tranne se stesso e chiunque dipenda (anche indirettamente) da lui,
 * perché creerebbe un ciclo. Quelli già in `selected` restano sempre
 * disponibili, anche se Fatti, così si possono deselezionare.
 */
export function dependencyOptions<T extends Pick<Task, "id" | "status" | "depends_on">>(
  tasks: T[],
  selfId: string | null,
  selected: string[] = [],
): T[] {
  const blocked = new Set<string>();
  if (selfId !== null) {
    blocked.add(selfId);
    // Chiusura dei dipendenti di `selfId`.
    for (let grew = true; grew; ) {
      grew = false;
      for (const t of tasks) {
        if (!blocked.has(t.id) && (t.depends_on ?? []).some((d) => blocked.has(d))) {
          blocked.add(t.id);
          grew = true;
        }
      }
    }
  }
  return tasks.filter((t) => !blocked.has(t.id) && (t.status !== "done" || selected.includes(t.id)));
}

/** Repository selezionabili: quelli rilevati più quelli già usati dai task, la root per prima. */
export function repoOptions(tasks: Pick<Task, "repo">[], detected: string[]): string[] {
  const set = new Set<string>(detected);
  for (const t of tasks) set.add(t.repo || ROOT_REPO);
  return [...set].sort((a, b) => (a === ROOT_REPO ? -1 : b === ROOT_REPO ? 1 : a.localeCompare(b)));
}

/** Più di un repository: il campo repo e le etichette dei repo hanno senso. */
export function isMultiRepo(options: string[]): boolean {
  return options.length > 1;
}

/** Repository del form → valore di `task_create` / `task_update` (la root è null). */
export function repoValue(repo: string): string | null {
  return repo === ROOT_REPO || repo === "" ? null : repo;
}

/** Valori iniziali del form di modifica. */
export function formFromTask(task: Task): TaskFormValues {
  return {
    title: task.title,
    task: task.task,
    depends_on: [...(task.depends_on ?? [])],
    model: task.model ?? "",
    repo: task.repo || ROOT_REPO,
  };
}

function sameSet(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((x) => b.includes(x));
}

/**
 * Patch dei soli campi cambiati rispetto al task. `includeRepo` = il campo repo
 * è modificabile (multi-repo, senza worktree). Vuota = niente da salvare.
 */
export function buildPatch(task: Task, v: TaskFormValues, includeRepo: boolean): TaskPatch {
  const patch: TaskPatch = {};
  const title = v.title.trim();
  if (title !== task.title) patch.title = title;
  const spec = v.task.trim();
  if (spec !== task.task.trim()) patch.task = spec;
  if (!sameSet(v.depends_on, task.depends_on ?? [])) patch.depends_on = v.depends_on;
  const model = sanitizeModel(v.model);
  if (model !== (task.model ?? null)) patch.model = model;
  if (includeRepo) {
    const repo = repoValue(v.repo);
    if (repo !== (task.repo || null)) patch.repo = repo;
  }
  return patch;
}

/**
 * Task appena creato nella board restituita da `task_create`: quello con il
 * titolo dato che non c'era prima (il più recente se ce ne fossero più d'uno).
 */
export function findCreatedTask(
  board: Pick<BoardView, "tasks">,
  title: string,
  previousIds: ReadonlySet<string>,
): Task | null {
  const wanted = title.trim();
  const matches = board.tasks.filter((t) => t.title === wanted);
  const fresh = matches.filter((t) => !previousIds.has(t.id));
  const pool = fresh.length > 0 ? fresh : matches;
  let best: Task | null = null;
  for (const t of pool) if (!best || t.created_at >= best.created_at) best = t;
  return best;
}
