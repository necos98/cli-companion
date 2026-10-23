import { api } from "../../lib/tauri";
import type { Workspace } from "../../lib/tauri";
import {
  DELIVERY_MODES,
  MERGE_METHODS,
  deliveryHelp,
  deliveryLabel,
  effectiveDelivery,
  mergeMethodLabel,
  normalizeDelivery,
  usesBranches,
} from "../../lib/board";
import type { Delivery, DeliveryMode, MergeMethod } from "../../lib/board";
import { cn } from "../../lib/utils";
import { Field, Section, SettingsPage, selectClass, useWorkspaceOp } from "./shared";

interface Props {
  workspace: Workspace;
  onWorkspaceUpdated: (ws: Workspace) => void;
}

/** Workspace › Consegna: come i worker consegnano il lavoro finito (docs/BOARD.MD). */
export default function DeliveryPage({ workspace, onWorkspaceUpdated }: Props) {
  const { run, saving, error } = useWorkspaceOp(onWorkspaceUpdated);
  const delivery = effectiveDelivery(workspace);

  // Ogni cambio salva subito la consegna intera; il backend la normalizza.
  const save = (patch: Partial<Delivery>) =>
    void run(() =>
      api.setDelivery(workspace.id, normalizeDelivery({ ...delivery, ...patch })),
    );

  return (
    <SettingsPage
      title="Consegna"
      description="Come un worker consegna il lavoro finito. Vale per i task che partono da ora; quelli già in corso non cambiano."
    >
      <Section>
        <Field label="Modalità">
          <div role="radiogroup" aria-label="Modalità di consegna" className="flex flex-col gap-1.5">
            {DELIVERY_MODES.map((mode: DeliveryMode) => (
              <label
                key={mode}
                className={cn(
                  "flex cursor-pointer items-start gap-2 rounded-md border px-2.5 py-2 transition-colors",
                  delivery.mode === mode
                    ? "border-primary/50 bg-surface-2"
                    : "border-border-strong hover:bg-surface-2/50",
                  saving && "cursor-not-allowed opacity-60",
                )}
              >
                <input
                  type="radio"
                  name={`delivery-mode-${workspace.id}`}
                  checked={delivery.mode === mode}
                  disabled={saving}
                  onChange={() => save({ mode })}
                  className="mt-0.5 h-3.5 w-3.5 accent-primary"
                />
                <span className="flex min-w-0 flex-col">
                  <span className="text-[13px] font-medium">{deliveryLabel(mode)}</span>
                  <span className="text-[11px] text-muted-foreground">{deliveryHelp(mode)}</span>
                </span>
              </label>
            ))}
          </div>
        </Field>

        {usesBranches(delivery.mode) && (
          <Field
            label="Worker in parallelo (worktree)"
            help="Acceso: ogni task lavora in un worktree dedicato e più task girano insieme (fino al limite di worker della coda). Spento: un solo worker per repository, nel checkout del repository."
          >
            <label className="flex items-center gap-2 text-[13px]">
              <input
                type="checkbox"
                checked={delivery.parallel}
                disabled={saving}
                onChange={(e) => save({ parallel: e.target.checked })}
                className="h-3.5 w-3.5 accent-primary"
              />
              Un worktree per task
            </label>
          </Field>
        )}

        {delivery.mode === "pr" && (
          <Field label="Metodo di merge" help="Come la pull request viene unita alla branch base.">
            <select
              value={delivery.merge_method}
              disabled={saving}
              onChange={(e) => save({ merge_method: e.target.value as MergeMethod })}
              className={selectClass}
            >
              {MERGE_METHODS.map((m) => (
                <option key={m} value={m}>
                  {mergeMethodLabel(m)}
                </option>
              ))}
            </select>
          </Field>
        )}
      </Section>

      <Section>
        <p className="text-xs font-semibold">Prerequisiti</p>
        <ul className="flex list-disc flex-col gap-1 pl-4 text-[11px] text-muted-foreground">
          <li
            className={cn(delivery.mode === "pr" && "text-foreground")}
          >
            <span className="font-medium">Branch + PR + merge</span>: serve la CLI <code className="font-mono">gh</code>{" "}
            autenticata nell’ambiente del workspace (<code className="font-mono">gh auth status</code>).
          </li>
          <li className={cn(delivery.mode === "local_merge" && "text-foreground")}>
            <span className="font-medium">Branch + merge locale</span>: il checkout del repository deve
            stare sulla branch base finché la coda lavora, altrimenti il task fallisce all’avvio.
          </li>
          <li>
            Le modalità con branch e il parallelo richiedono un repository git; la branch base si
            sceglie per repository in Repository.
          </li>
        </ul>
      </Section>

      {error && <p className="text-xs text-destructive">{error}</p>}
    </SettingsPage>
  );
}
