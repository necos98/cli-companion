// Utilizzo del coding agent: limiti di abbonamento (5 ore / settimana) e token
// per sessione. Logica pura, a zero dipendenze: importabile dall'app e dai
// test `node --test`. I tipi sono strutturali (compatibili con quelli di tauri.ts).

export interface LimitWindowLike {
  used_percentage: number;
  resets_at: string | null;
}

export interface UsageLimitsLike {
  state: "ok" | "expired" | "unavailable" | "disabled" | "unsupported";
  five_hour: LimitWindowLike | null;
  seven_day: LimitWindowLike | null;
  message: string | null;
}

export interface TokenTotalsLike {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
}

export interface SessionUsageLike {
  main: TokenTotalsLike;
  subagents: TokenTotalsLike;
  context_tokens: number;
  model: string | null;
  messages: number;
}

// ---------------------------------------------------------------------------
// Formattazione
// ---------------------------------------------------------------------------

function oneDecimal(x: number): string {
  return (Math.round(x * 10) / 10).toString().replace(".", ",");
}

/** Numero compatto: `950`, `1,2k`, `12k`, `1,2M`, `12M`. */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) {
    const k = n / 1000;
    if (k < 9.95) return `${oneDecimal(k)}k`;
    if (Math.round(k) < 1000) return `${Math.round(k)}k`;
  }
  const m = n / 1_000_000;
  return m < 9.95 ? `${oneDecimal(m)}M` : `${Math.round(m)}M`;
}

export type LimitSeverity = "normal" | "warning" | "critical";

/** `< 80` normale, `>= 80` attenzione, `>= 100` critico. */
export function limitSeverity(percentage: number): LimitSeverity {
  if (percentage >= 100) return "critical";
  if (percentage >= 80) return "warning";
  return "normal";
}

const pad2 = (n: number) => String(n).padStart(2, "0");

/** Reset: oggi `HH:MM` (ora locale), altrimenti `DD/MM`; assente/non valido = `null`. */
export function formatReset(iso: string | null | undefined, now: Date = new Date()): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  return sameDay
    ? `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
    : `${pad2(d.getDate())}/${pad2(d.getMonth() + 1)}`;
}

/** Data e ora complete del reset, per il tooltip. */
export function formatResetFull(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  return `${pad2(d.getDate())}/${pad2(d.getMonth() + 1)}/${d.getFullYear()} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

// ---------------------------------------------------------------------------
// Barra di stato
// ---------------------------------------------------------------------------

export interface LimitItem {
  key: "5h" | "7d" | "expired";
  text: string;
  /** `muted` per il suggerimento "apri Claude Code". */
  severity: LimitSeverity | "muted";
  title: string;
}

function windowItem(
  key: "5h" | "7d",
  short: string,
  long: string,
  w: LimitWindowLike,
  now: Date,
): LimitItem {
  const pct = Math.round(w.used_percentage);
  const reset = formatReset(w.resets_at, now);
  const full = formatResetFull(w.resets_at);
  return {
    key,
    text: `${short} ${pct}%${reset ? ` · ${reset}` : ""}`,
    severity: limitSeverity(w.used_percentage),
    title: `${long}: ${oneDecimal(w.used_percentage)}% usato${full ? `\nSi azzera il ${full}` : ""}`,
  };
}

/** Voci da mostrare per la risposta di un ambiente (vuoto = nascosto). */
export function limitItems(reply: UsageLimitsLike | null | undefined, now: Date = new Date()): LimitItem[] {
  if (!reply) return [];
  if (reply.state === "expired") {
    return [
      {
        key: "expired",
        text: "5h: apri Claude Code",
        severity: "muted",
        title: reply.message?.trim() || "Accesso scaduto: apri Claude Code per rinnovarlo.",
      },
    ];
  }
  if (reply.state !== "ok") return [];
  const items: LimitItem[] = [];
  if (reply.five_hour) items.push(windowItem("5h", "5h", "Limite delle 5 ore", reply.five_hour, now));
  if (reply.seven_day) items.push(windowItem("7d", "sett.", "Limite settimanale", reply.seven_day, now));
  return items;
}

export interface LimitGroup {
  envKey: string;
  /** Prefisso con l'etichetta dell'ambiente, solo se ne compaiono più d'uno. */
  prefix: string | null;
  items: LimitItem[];
}

/** Gruppi per ambiente; il prefisso c'è solo quando i gruppi non vuoti sono più di uno. */
export function limitGroups(
  entries: Array<{ envKey: string; label: string; reply: UsageLimitsLike | null | undefined }>,
  now: Date = new Date(),
): LimitGroup[] {
  const groups = entries
    .map((e) => ({ envKey: e.envKey, label: e.label, items: limitItems(e.reply, now) }))
    .filter((g) => g.items.length > 0);
  return groups.map((g) => ({
    envKey: g.envKey,
    prefix: groups.length > 1 ? g.label : null,
    items: g.items,
  }));
}

/** Testo di un gruppo: `WIN 5h 45% · 14:20 │ sett. 2% · 28/10`. */
export function limitGroupText(group: LimitGroup): string {
  const body = group.items.map((i) => i.text).join(" │ ");
  return group.prefix ? `${group.prefix} ${body}` : body;
}

// ---------------------------------------------------------------------------
// Intestazione del pannello
// ---------------------------------------------------------------------------

const sumInput = (t: TokenTotalsLike) =>
  t.input_tokens + t.cache_read_tokens + t.cache_creation_tokens;

/** `↑ 1,2M ↓ 45k · ctx 82k` (input comprensivo della cache). */
export function usageText(u: SessionUsageLike): string {
  return `↑ ${formatTokens(sumInput(u.main))} ↓ ${formatTokens(u.main.output_tokens)} · ctx ${formatTokens(u.context_tokens)}`;
}

/** Tooltip su più righe con il dettaglio dei token. */
export function usageTooltip(u: SessionUsageLike): string {
  const lines = [
    `Input: ${formatTokens(u.main.input_tokens)}`,
    `Output: ${formatTokens(u.main.output_tokens)}`,
    `Cache letta: ${formatTokens(u.main.cache_read_tokens)}`,
    `Cache scritta: ${formatTokens(u.main.cache_creation_tokens)}`,
    `Contesto: ${formatTokens(u.context_tokens)}`,
  ];
  if (u.model) lines.push(`Modello: ${u.model}`);
  lines.push(`Messaggi: ${u.messages}`);
  const s = u.subagents;
  if (sumInput(s) + s.output_tokens > 0) {
    lines.push(
      `Subagent: ↑ ${formatTokens(sumInput(s))} ↓ ${formatTokens(s.output_tokens)}`,
    );
  }
  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Politica di polling
// ---------------------------------------------------------------------------

/** Limiti: ogni 3 minuti. */
export const LIMITS_POLL_MS = 180_000;
/** Utilizzo di sessione: ogni 20 s mentre l'agente lavora. */
export const SESSION_POLL_MS = 20_000;

/** Eventi dopo i quali si rilegge l'utilizzo della sessione. */
export function shouldRefreshSessionUsage(event: string): boolean {
  return event === "finished" || event === "needs-input" || event === "session-start";
}

/** Intervallo di polling per la fase del tab (`null` = nessun polling periodico). */
export function sessionPollInterval(phase: string | undefined): number | null {
  return phase === "running" ? SESSION_POLL_MS : null;
}

/** Chiave dell'ambiente di un workspace: "windows" o il nome della distro WSL. */
export function workspaceEnvKey(ws: { kind: string; distro?: string | null }): string {
  return ws.kind === "wsl" && ws.distro ? ws.distro : "windows";
}
