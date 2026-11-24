// Il Bot di un task nell'interfaccia: scelta nei form, badge e modello da
// tenere cambiando Bot. Funzioni pure a zero dipendenze (solo i tipi dei
// bindings), importabili sia dall'app sia dai test `node --test`. Niente
// risoluzione di agente, modello, effort o skill: la fa il backend quando il
// task si salva (`board::agent::resolve_spec`), qui si mostra il risultato.

import type { AgentBot } from "../bindings/AgentBot.ts";
import type { TaskKind } from "../bindings/TaskKind.ts";
import { PLAN_LABEL } from "./plan.ts";
import { REVIEW_LABEL } from "./review.ts";

/** Il Bot per il lavoro senza specializzazione (stesso id dei preset del backend). */
export const GENERAL_BOT_ID = "general";

/** Voce del selettore per un task senza Bot (task vecchi). */
export const NO_BOT_LABEL = "Nessun Bot";

/** Indicazione accanto a un Bot che non si può più scegliere (disabilitato o cancellato). */
export const UNSELECTABLE_BOT_SUFFIX = "(non disponibile)";

/** Testo di un task senza Bot (task vecchi). */
export const NO_BOT_TEXT = "—";

export interface BotOption {
  id: string;
  label: string;
}

/** Il Bot `id`, se esiste ancora. */
export function botById(bots: readonly AgentBot[], id: string | undefined): AgentBot | undefined {
  return id ? bots.find((b) => b.id === id) : undefined;
}

/** Nome del Bot `id` (l'id stesso se è stato cancellato); `null` per un task senza Bot. */
export function botName(bots: readonly AgentBot[], id: string | undefined): string | null {
  if (!id) return null;
  return botById(bots, id)?.name ?? id;
}

/** Tipi di task nell'ordine in cui si mostrano (quello di `TaskKind::ALL`). */
export const TASK_KINDS: readonly TaskKind[] = ["work", "review", "plan"];

/** Etichette dei tipi di task, le stesse dei badge. */
export const TASK_KIND_LABELS: Readonly<Record<TaskKind, string>> = {
  work: "Lavoro",
  review: REVIEW_LABEL,
  plan: PLAN_LABEL,
};

/** Riepilogo di un Bot senza tipi di task: parte solo dai terminali. */
export const INTERACTIVE_ONLY_LABEL = "Solo terminali interattivi";

/** Dove si abilita un tipo di task a un Bot (rimando dei messaggi). */
export const BOT_SETTINGS_PATH = "Impostazioni › Bot";

/** Il Bot esegue i task di tipo `kind`. */
export function botHasKind(bot: Pick<AgentBot, "kinds">, kind: TaskKind): boolean {
  return bot.kinds.includes(kind);
}

/** I Bot selezionabili per un task di tipo `kind`: abilitati e con quel tipo. */
export function botsForKind(bots: readonly AgentBot[], kind: TaskKind): AgentBot[] {
  return bots.filter((b) => b.enabled && botHasKind(b, kind));
}

/** Tipi di task di un Bot, per il riepilogo: «Lavoro, Review» o `INTERACTIVE_ONLY_LABEL`. */
export function kindsSummary(kinds: readonly TaskKind[]): string {
  const labels = TASK_KINDS.filter((k) => kinds.includes(k)).map((k) => TASK_KIND_LABELS[k]);
  return labels.length === 0 ? INTERACTIVE_ONLY_LABEL : labels.join(", ");
}

/**
 * Voci del selettore per un task di tipo `kind`: i Bot abilitati che lo
 * eseguono e, se `selected` non è fra questi (Bot disabilitato, cancellato o
 * non più adatto al tipo), la voce corrente in coda, così il campo mostra
 * sempre un valore vero. "Nessun Bot" c'è solo per chi non ne ha uno: ai task
 * con Bot non si toglie.
 */
export function botOptions(bots: readonly AgentBot[], selected: string, kind: TaskKind): BotOption[] {
  const options = botsForKind(bots, kind).map((b) => ({ id: b.id, label: b.name }));
  if (selected === "") return [{ id: "", label: NO_BOT_LABEL }, ...options];
  if (!options.some((o) => o.id === selected)) {
    options.push({ id: selected, label: `${botName(bots, selected)} ${UNSELECTABLE_BOT_SUFFIX}` });
  }
  return options;
}

/**
 * Bot preselezionato in un task di tipo `kind`: `general` se lo esegue,
 * altrimenti il primo che lo esegue; vuoto se non ce ne sono.
 */
export function defaultBotChoice(bots: readonly AgentBot[], kind: TaskKind): string {
  const usable = botsForKind(bots, kind);
  return (usable.find((b) => b.id === GENERAL_BOT_ID) ?? usable[0])?.id ?? "";
}

export interface BotKindChange {
  /** Il Bot da tenere dopo il cambio di tipo. */
  bot: string;
  /** Avviso in linea se il Bot è stato sostituito; `null` se è rimasto. */
  notice: string | null;
}

/**
 * Cambio di tipo nel form: il Bot scelto resta se esegue il nuovo tipo (o se il
 * task non ne ha uno), altrimenti si passa al default per il tipo e lo si dice.
 */
export function botAfterKindChange(bots: readonly AgentBot[], current: string, kind: TaskKind): BotKindChange {
  if (current === "") return { bot: current, notice: null };
  const bot = botById(bots, current);
  if (bot?.enabled && botHasKind(bot, kind)) return { bot: current, notice: null };
  const next = defaultBotChoice(bots, kind);
  const base = `${botName(bots, current)} non esegue i task di tipo ${TASK_KIND_LABELS[kind]}`;
  return { bot: next, notice: next === "" ? `${base}.` : `${base}: scelto ${botName(bots, next)}.` };
}

/**
 * Errore del campo Bot: ci sono dei Bot ma nessuno abilitato esegue il tipo del
 * task, quindi il backend rifiuterebbe l'invio. Senza Bot (task vecchi, elenco
 * non ancora caricato) non si blocca nulla.
 */
export function botKindError(bots: readonly AgentBot[], kind: TaskKind): string | undefined {
  if (bots.length === 0 || botsForKind(bots, kind).length > 0) return undefined;
  return `Nessun Bot abilitato esegue i task di tipo ${TASK_KIND_LABELS[kind]}: abilitane uno in ${BOT_SETTINGS_PATH}.`;
}

/**
 * Agente del Bot `id` per i suggerimenti del modello: il suo, altrimenti quello
 * del workspace. Vuoto se il Bot non c'è.
 */
export function botAgentId(bots: readonly AgentBot[], id: string, workspaceAgentId: string | undefined): string {
  const bot = botById(bots, id);
  return bot ? (bot.agent ?? workspaceAgentId ?? "") : "";
}
