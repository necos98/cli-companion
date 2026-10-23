// Nome del modello di un worker o di un task: normalizzazione e validazione.
// Funzioni pure a zero dipendenze, importabili sia dall'app sia dai test.

/** Lunghezza massima del nome di un modello. */
export const MODEL_MAX = 64;

/** Come `valid_model` del backend: lettere, cifre, '.', '_', '-' (1-64 caratteri). */
const MODEL_RE = /^[A-Za-z0-9._-]{1,64}$/;

/** Modello normalizzato: trim, `null` se vuoto. */
export function sanitizeModel(raw: string): string | null {
  const m = raw.trim().slice(0, MODEL_MAX);
  return m === "" ? null : m;
}

/** Errore sul nome del modello o null (vuoto = valido: predefinito). */
export function modelError(raw: string): string | null {
  const m = raw.trim();
  if (m === "") return null;
  return MODEL_RE.test(m)
    ? null
    : `Il modello deve avere 1-${MODEL_MAX} caratteri tra lettere, cifre, '.', '_' e '-'`;
}
