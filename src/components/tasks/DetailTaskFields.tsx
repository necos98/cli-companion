import type { ReactNode } from "react";
import DetailDependencyPicker from "./DetailDependencyPicker";
import { Input } from "../ui/input";
import { selectClass } from "../settings/shared";
import { dependencyOptions } from "../../lib/taskForm";
import type { TaskFormErrors, TaskFormValues } from "../../lib/taskForm";
import type { Task } from "../../lib/board";
import { MODEL_MAX } from "../../lib/model";
import { REVIEW_OPTION_HELP, REVIEW_OPTION_LABEL } from "../../lib/review";
import { cn } from "../../lib/utils";

interface Props {
  /** Prefisso degli id dei campi (più form nella stessa pagina). */
  idPrefix: string;
  values: TaskFormValues;
  onChange: (patch: Partial<TaskFormValues>) => void;
  errors: TaskFormErrors;
  /** Tutti i task della board: le dipendenze possibili ne derivano. */
  tasks: Task[];
  /** Task in modifica (esclude se stesso e i cicli); null = task nuovo. */
  selfId: string | null;
  /** Repository scelti dal form; meno di due = campo nascosto. */
  repoChoices: string[];
  /** Il repo non si cambia (il task ha già un worktree). */
  repoLocked?: boolean;
  modelPlaceholder: string;
  disabled: boolean;
  specRows?: number;
}

function FieldBox({
  id,
  label,
  error,
  help,
  children,
}: {
  id: string;
  label: string;
  error?: string;
  help?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-xs font-medium">
        {label}
      </label>
      {children}
      {error ? (
        <p id={`${id}-error`} role="alert" className="text-[11px] text-destructive">
          {error}
        </p>
      ) : (
        help && <p className="text-[11px] text-muted-foreground">{help}</p>
      )}
    </div>
  );
}

/** Campi condivisi dal dialog "Nuovo task" e dalla modifica nel dettaglio. */
export default function DetailTaskFields({
  idPrefix,
  values,
  onChange,
  errors,
  tasks,
  selfId,
  repoChoices,
  repoLocked = false,
  modelPlaceholder,
  disabled,
  specRows = 8,
}: Props) {
  const options = dependencyOptions(tasks, selfId, values.depends_on);
  const id = (name: string) => `${idPrefix}-${name}`;
  return (
    <>
      <FieldBox id={id("title")} label="Titolo" error={errors.title}>
        <Input
          id={id("title")}
          value={values.title}
          disabled={disabled}
          maxLength={200}
          aria-invalid={errors.title ? true : undefined}
          aria-describedby={errors.title ? `${id("title")}-error` : undefined}
          onChange={(e) => onChange({ title: e.target.value })}
        />
      </FieldBox>

      <FieldBox
        id={id("task")}
        label="Specifica"
        error={errors.task}
        help="Obiettivo, contesto, vincoli, “fatto quando” e comandi di verifica: il worker non vede nient’altro. Markdown."
      >
        <textarea
          id={id("task")}
          value={values.task}
          disabled={disabled}
          rows={specRows}
          aria-invalid={errors.task ? true : undefined}
          aria-describedby={errors.task ? `${id("task")}-error` : undefined}
          onChange={(e) => onChange({ task: e.target.value })}
          className={cn(
            "w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 font-mono text-[12px] leading-relaxed outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
          )}
        />
      </FieldBox>

      <div className="flex flex-col gap-1">
        <label htmlFor={id("kind")} className="flex items-center gap-2 text-xs font-medium">
          <input
            id={id("kind")}
            type="checkbox"
            checked={values.kind === "review"}
            disabled={disabled}
            onChange={(e) => onChange({ kind: e.target.checked ? "review" : "work" })}
          />
          {REVIEW_OPTION_LABEL}
        </label>
        <p className="text-[11px] text-muted-foreground">{REVIEW_OPTION_HELP}</p>
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="text-xs font-medium" id={id("deps-label")}>
          Dipende da
        </span>
        <DetailDependencyPicker
          idPrefix={idPrefix}
          options={options}
          selected={values.depends_on}
          onChange={(depends_on) => onChange({ depends_on })}
          disabled={disabled}
        />
        {errors.depends_on && (
          <p id={id("deps-error")} role="alert" className="text-[11px] text-destructive">
            {errors.depends_on}
          </p>
        )}
      </div>

      <FieldBox
        id={id("model")}
        label="Modello"
        error={errors.model}
        help="Vuoto: il modello predefinito dei worker del workspace."
      >
        <Input
          id={id("model")}
          value={values.model}
          disabled={disabled}
          maxLength={MODEL_MAX + 16}
          placeholder={modelPlaceholder}
          aria-invalid={errors.model ? true : undefined}
          aria-describedby={errors.model ? `${id("model")}-error` : undefined}
          onChange={(e) => onChange({ model: e.target.value })}
        />
      </FieldBox>

      {repoChoices.length > 1 && (
        <FieldBox
          id={id("repo")}
          label="Repository"
          help={repoLocked ? "Il task ha già un worktree: il repository non si cambia." : undefined}
        >
          <select
            id={id("repo")}
            value={values.repo}
            disabled={disabled || repoLocked}
            onChange={(e) => onChange({ repo: e.target.value })}
            className={selectClass}
          >
            {repoChoices.map((r) => (
              <option key={r} value={r}>
                {r === "." ? ". (root del workspace)" : r}
              </option>
            ))}
          </select>
        </FieldBox>
      )}
    </>
  );
}
