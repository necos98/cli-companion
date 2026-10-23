// Fasce orarie globali (gate dello scheduler dei task, docs/BOARD.MD). Tipi del
// contratto, validazione, normalizzazione e testi di stato. Funzioni pure a
// zero dipendenze, importabili sia dall'app sia dai test `node --test`.
// La logica "finestra aperta / prossimo cambio" e' del backend (schedule.rs):
// qui si mostra soltanto ciò che comunica (`window_open`, `next_change_at`).

/** Una fascia oraria locale `[start, end)`, formato `HH:MM`. */
export interface ScheduleWindow {
  start: string;
  end: string;
}

/** Configurazione globale (`schedule.json`, serde snake_case). */
export interface Schedule {
  enabled: boolean;
  windows: ScheduleWindow[];
  keep_awake: boolean;
}

/** Risposta di `schedule_get` / `schedule_set` e payload di `schedule-state`. */
export interface ScheduleState {
  schedule: Schedule;
  window_open: boolean;
  /** Unix ms del prossimo cambio di stato della finestra, o null. */
  next_change_at: number | null;
}

/** Numero massimo di fasce. */
export const MAX_WINDOWS = 12;

/** Fascia proposta da "Aggiungi": la notte, fuori dalle ore di punta. */
export const DEFAULT_WINDOW: ScheduleWindow = { start: "20:00", end: "13:00" };

export const EMPTY_SCHEDULE: Schedule = { enabled: false, windows: [], keep_awake: true };

const TIME_RE = /^(\d{1,2}):(\d{2})$/;

/** Minuti dalla mezzanotte di un orario `H:MM`/`HH:MM`, o null se non valido. */
export function parseTime(text: string): number | null {
  const m = TIME_RE.exec(text.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

export function isValidTime(text: string): boolean {
  return parseTime(text) !== null;
}

/** Orario canonico `HH:MM` ("9:05" -> "09:05"); invariato (trim) se non valido. */
export function normalizeTime(text: string): string {
  const t = text.trim();
  const mins = parseTime(t);
  if (mins === null) return t;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/** Una fascia con orari canonici. */
export function normalizeWindow(w: ScheduleWindow): ScheduleWindow {
  return { start: normalizeTime(w.start), end: normalizeTime(w.end) };
}

/** Errore di una fascia (testo italiano) o null se valida. */
export function windowError(w: ScheduleWindow): string | null {
  const s = parseTime(w.start);
  const e = parseTime(w.end);
  if (s === null || e === null) return "Orario non valido (usa HH:MM)";
  if (s === e) return "Inizio e fine devono essere diversi";
  return null;
}

/** La fascia attraversa la mezzanotte (`start > end`)? Falso se non valida. */
export function crossesMidnight(w: ScheduleWindow): boolean {
  const s = parseTime(w.start);
  const e = parseTime(w.end);
  return s !== null && e !== null && s > e;
}

/** Descrizione breve di una fascia, con nota se attraversa la mezzanotte. */
export function windowDescription(w: ScheduleWindow): string {
  const base = `${normalizeTime(w.start)} – ${normalizeTime(w.end)}`;
  return crossesMidnight(w) ? `${base} (fino al giorno dopo)` : base;
}

/** Errore dell'intera configurazione (primo problema trovato) o null. */
export function scheduleError(s: Schedule): string | null {
  if (s.windows.length > MAX_WINDOWS) return `Al massimo ${MAX_WINDOWS} fasce`;
  for (let i = 0; i < s.windows.length; i++) {
    const err = windowError(s.windows[i]);
    if (err) return `Fascia ${i + 1}: ${err}`;
  }
  if (s.enabled && s.windows.length === 0) return "Aggiungi almeno una fascia oraria";
  return null;
}

/** Configurazione pronta da inviare: orari canonici. */
export function normalizeSchedule(s: Schedule): Schedule {
  return {
    enabled: s.enabled,
    keep_awake: s.keep_awake,
    windows: s.windows.map(normalizeWindow),
  };
}

/** Aggiunge una fascia (no-op oltre il massimo). */
export function addWindow(windows: ScheduleWindow[]): ScheduleWindow[] {
  if (windows.length >= MAX_WINDOWS) return windows;
  return [...windows, { ...DEFAULT_WINDOW }];
}

export function removeWindow(windows: ScheduleWindow[], index: number): ScheduleWindow[] {
  return windows.filter((_, i) => i !== index);
}

export function updateWindow(
  windows: ScheduleWindow[],
  index: number,
  patch: Partial<ScheduleWindow>,
): ScheduleWindow[] {
  return windows.map((w, i) => (i === index ? { ...w, ...patch } : w));
}

/** Le due configurazioni sono equivalenti (dopo la normalizzazione)? */
export function sameSchedule(a: Schedule, b: Schedule): boolean {
  return JSON.stringify(normalizeSchedule(a)) === JSON.stringify(normalizeSchedule(b));
}

/** Orario locale `HH:MM` di un istante in ms. */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/**
 * "20:00", oppure "20:00 di domani" / "20:00 del 12/10" se l'istante non e'
 * nello stesso giorno di `now`.
 */
export function formatChange(ms: number, now: number = Date.now()): string {
  const clock = formatClock(ms);
  const d = new Date(ms);
  const n = new Date(now);
  const dayDiff = Math.round(
    (new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() -
      new Date(n.getFullYear(), n.getMonth(), n.getDate()).getTime()) /
      86_400_000,
  );
  if (dayDiff === 0) return clock;
  if (dayDiff === 1) return `${clock} di domani`;
  return `${clock} del ${String(d.getDate()).padStart(2, "0")}/${String(d.getMonth() + 1).padStart(2, "0")}`;
}

/** Riga di stato delle impostazioni globali. */
export function scheduleStatusText(
  state: Pick<ScheduleState, "schedule" | "window_open" | "next_change_at"> | null,
  now: number = Date.now(),
): string {
  if (!state || !state.schedule.enabled) return "Esecuzione automatica disattivata";
  if (state.window_open) {
    return state.next_change_at === null
      ? "Finestra aperta"
      : `Finestra aperta fino alle ${formatChange(state.next_change_at, now)}`;
  }
  return state.next_change_at === null
    ? "Nessuna finestra in programma"
    : `Prossima apertura alle ${formatChange(state.next_change_at, now)}`;
}
