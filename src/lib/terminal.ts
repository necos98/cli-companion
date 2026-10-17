// Stato terminale (titolo tab + semaforo busy/idle): utility pure.
// File volutamente a zero dipendenze: importabile sia dall'app sia dai test
// `node --test` (type-stripping), come src/lib/updater.ts.

/** Lunghezza massima del titolo mostrato nei tab. */
export const TERMINAL_TITLE_MAX = 40;

/**
 * Normalizza un titolo OSC ricevuto da xterm: trim, collassa gli spazi,
 * ignora le stringhe vuote (ritorna null), tronca a 40 caratteri.
 */
export function sanitizeTerminalTitle(raw: string): string | null {
  const collapsed = raw.trim().replace(/\s+/g, " ");
  if (collapsed === "") return null;
  return collapsed.slice(0, TERMINAL_TITLE_MAX);
}

/** Azione appunti decisa per un tasto premuto nel terminale. */
export type ClipboardKeyAction = "copy" | "paste" | null;

/**
 * Decide se un tasto premuto nel terminale è un'azione appunti:
 * Ctrl+C con selezione, Ctrl+Shift+C o Ctrl+Ins = copia;
 * Ctrl+V, Ctrl+Shift+V o Shift+Ins = incolla; tutto il resto = null.
 * Ctrl+C senza selezione resta null così arriva alla shell come SIGINT.
 */
export function clipboardKeyAction(
  e: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey">,
  hasSelection: boolean,
): ClipboardKeyAction {
  if (e.key === "Insert") {
    if (e.ctrlKey && !e.shiftKey) return "copy";
    if (e.shiftKey && !e.ctrlKey) return "paste";
    return null;
  }
  if (!e.ctrlKey) return null;
  const key = e.key.toLowerCase();
  if (key === "c" && (e.shiftKey || hasSelection)) return "copy";
  if (key === "v") return "paste";
  return null;
}

/**
 * Ctrl+V grezzo (SYN, U+0016): è il byte che xterm invia al PTY per Ctrl+V
 * quando il tasto non è intercettato. Le CLI (Claude Code ecc.) lo usano come segnale
 * per prelevare un'immagine dagli appunti di sistema.
 */
export const PASTE_FALLBACK_KEY = "\x16";

/** Decisione presa dopo aver letto gli appunti per un'azione "incolla". */
export type PasteResolution =
  | { kind: "paste"; text: string }
  | { kind: "forward-key"; key: string };

/**
 * Decide cosa fare con il contenuto testuale degli appunti dopo un gesto
 * di incolla: testo non vuoto -> incollalo nel terminale; testo vuoto
 * (appunti con sola immagine, o vuoti) -> inoltra Ctrl+V grezzo al PTY
 * così la CLI può gestire l'incolla da sola, come prima della v0.3.1.
 */
export function resolvePasteAction(clipboardText: string): PasteResolution {
  if (clipboardText !== "") return { kind: "paste", text: clipboardText };
  return { kind: "forward-key", key: PASTE_FALLBACK_KEY };
}

export interface BusyTracker {
  /** Segnala output arrivato dal pty: diventa busy. */
  markOutput: () => void;
  /** Segnala Invio premuto nel terminale: diventa busy. */
  markEnter: () => void;
  /** Forza idle (es. uscita del pty), notificando solo su cambio. */
  markIdle: () => void;
  /** Ferma il timer interno. */
  dispose: () => void;
}

/**
 * Semaforo busy/idle con debounce: busy=true su output/enter, busy=false
 * dopo `idleMs` di silenzio. `onChange` è chiamata solo sul cambio di stato.
 */
export function createBusyTracker(opts: {
  idleMs: number;
  onChange: (busy: boolean) => void;
}): BusyTracker {
  const { idleMs, onChange } = opts;
  let busy = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const setBusy = (next: boolean) => {
    if (next === busy) return;
    busy = next;
    onChange(next);
  };

  const poke = () => {
    setBusy(true);
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      setBusy(false);
    }, idleMs);
  };

  const stopTimer = () => {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  };

  return {
    markOutput: poke,
    markEnter: poke,
    markIdle: () => {
      stopTimer();
      setBusy(false);
    },
    dispose: stopTimer,
  };
}
