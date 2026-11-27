// Impostazioni dell'Agents HUB (docs/HUB.MD): limiti del backend, riepiloghi
// degli accordion e validazione di ciò che l'utente scrive prima dell'invio.
// Il backend ripete ogni controllo: qui servono a dare subito un messaggio
// chiaro. Funzioni pure a zero dipendenze, importabili sia dall'app sia dai test.

import type { HubLanguage } from "../bindings/HubLanguage.ts";
import type { HubSecretary } from "../bindings/HubSecretary.ts";
import type { HubSecretaryTool } from "../bindings/HubSecretaryTool.ts";
import type { HubSettingsView } from "../bindings/HubSettingsView.ts";
import { SECRETARY_NAME_MAX, isConfigured } from "./hubSecretaries.ts";

/** Limiti del backend (`hub/settings/*.rs`, `hub/api_key.rs`). */
export const IDLE_MINUTES_RANGE = { min: 1, max: 240 } as const;
export const MAX_STEPS_RANGE = { min: 1, max: 20 } as const;
export const BASE_URL_MAX = 2048;
export const MODEL_MAX = 128;
export const API_KEY_MAX = 1024;
export const SYSTEM_PROMPT_MAX = 8000;
export const SECRETARIES_MAX = 20;

export type CheckResult<T> = { ok: true; value: T } | { ok: false; reason: string };

const ok = <T>(value: T): CheckResult<T> => ({ ok: true, value });
const fail = <T>(reason: string): CheckResult<T> => ({ ok: false, reason });

const charCount = (text: string): number => [...text].length;

// ---------------------------------------------------------------- lingua e tool

export const LANGUAGES: readonly { id: HubLanguage; label: string }[] = [
  { id: "it", label: "Italiano" },
  { id: "en", label: "English" },
  { id: "es", label: "Español" },
  { id: "fr", label: "Français" },
  { id: "de", label: "Deutsch" },
];

/** Tool del motore, nell'ordine canonico del backend, con la descrizione breve mostrata all'utente. */
export const SECRETARY_TOOLS: readonly { id: HubSecretaryTool; description: string }[] = [
  { id: "board_status", description: "Legge task e stati della board" },
  {
    id: "board_overview",
    description: "Riepiloga in una volta tutti i workspace: conteggi, task e domande che aspettano te",
  },
  { id: "task_details", description: "Legge un task per intero: specifica, report, commit e domanda in attesa" },
  { id: "thread_read", description: "Legge il riassunto e i messaggi del thread, anche i più vecchi" },
  { id: "post_message", description: "Scrive nel thread" },
  { id: "open_plan_task", description: "Apre un task di pianificazione" },
  { id: "decide_proposal", description: "Accetta o scarta la proposta a cui rispondi" },
  { id: "relay_answer", description: "Risponde a un worker al posto tuo, quando glielo chiedi" },
  { id: "task_action", description: "Ritenta un task fallito o ne cambia la posizione nella coda" },
  { id: "board_control", description: "Mette in pausa o riprende la board e ne regola i task in parallelo" },
  {
    id: "request_task_action",
    description: "Ti chiede di fermare, eliminare o chiudere un task: lo esegui tu con un clic",
  },
  {
    id: "request_task_update",
    description: "Ti chiede di cambiare titolo, specifica, dipendenze o modello di un task: lo confermi tu con un clic",
  },
  { id: "set_thread_state", description: "Chiude o riapre questo thread, quando glielo chiedi" },
];

function languageLabel(language: HubLanguage): string {
  return LANGUAGES.find((l) => l.id === language)?.label ?? language;
}

// ---------------------------------------------------------------- riepiloghi

const LOCAL_HOSTS = new Set(["localhost", "0.0.0.0", "::1", "[::1]"]);
const LOOPBACK_V4 = /^127(\.\d{1,3}){3}$/;

/** L'endpoint punta a questo computer (localhost, 127.x, ::1). `null` se l'indirizzo non si legge. */
export function isLocalEndpoint(baseUrl: string): boolean | null {
  const text = baseUrl.trim();
  if (text === "") return null;
  let host: string;
  try {
    host = new URL(text).hostname.toLowerCase();
  } catch {
    return null;
  }
  return LOCAL_HOSTS.has(host) || LOOPBACK_V4.test(host) || host.endsWith(".localhost");
}

/** «Endpoint locale · API key impostata», «Endpoint remoto · nessuna API key», «Non configurato». */
export function modelSummary(settings: Pick<HubSettingsView, "provider" | "api_key_set">): string {
  const { base_url, model } = settings.provider;
  const local = isLocalEndpoint(base_url);
  if (local === null && model.trim() === "") return "Non configurato";
  const endpoint = local === null ? "Endpoint non valido" : local ? "Endpoint locale" : "Endpoint remoto";
  return `${endpoint} · ${settings.api_key_set ? "API key impostata" : "nessuna API key"}`;
}

/** «20 min · 8 passi». */
export function sessionSummary(session: HubSettingsView["session"]): string {
  return `${session.idle_minutes} min · ${session.max_steps} ${session.max_steps === 1 ? "passo" : "passi"}`;
}

/** «3 segretari · 2 configurati · italiano». */
export function secretariesSummary(settings: Pick<HubSettingsView, "secretaries" | "language">): string {
  const count = settings.secretaries.length;
  const language = languageLabel(settings.language).toLowerCase();
  if (count === 0) return `Nessun segretario · ${language}`;
  const configured = settings.secretaries.filter(isConfigured).length;
  const noun = count === 1 ? "segretario" : "segretari";
  const configuredNoun = configured === 1 ? "configurato" : "configurati";
  return `${count} ${noun} · ${configured} ${configuredNoun} · ${language}`;
}

// ---------------------------------------------------------------- modello

export interface ProviderInput {
  baseUrl: string;
  model: string;
}

/** Endpoint e modello da inviare: senza spazi ai bordi, nei limiti; entrambi vuoti = non configurato. */
export function checkProvider(baseUrl: string, model: string): CheckResult<ProviderInput> {
  const url = baseUrl.trim().replace(/\/+$/, "");
  const name = model.trim();
  if (charCount(url) > BASE_URL_MAX) return fail(`L’endpoint supera i ${BASE_URL_MAX} caratteri.`);
  if (charCount(name) > MODEL_MAX) return fail(`Il nome del modello supera i ${MODEL_MAX} caratteri.`);
  if (url !== "" && !/^https?:\/\//i.test(url)) return fail("L’endpoint deve iniziare con http:// o https://.");
  return ok({ baseUrl: url, model: name });
}

/** API key da inviare: non vuota e nel limite, senza spazi ai bordi (di solito incollati per sbaglio). */
export function checkApiKey(raw: string): CheckResult<string> {
  const key = raw.trim();
  if (key === "") return fail("Scrivi la API key.");
  if (charCount(key) > API_KEY_MAX) return fail(`La API key supera i ${API_KEY_MAX} caratteri.`);
  return ok(key);
}

// ---------------------------------------------------------------- sessioni

export interface SessionInput {
  idleMinutes: number;
  maxSteps: number;
}

function wholeInRange(raw: string, label: string, range: { min: number; max: number }): CheckResult<number> {
  const text = raw.trim();
  const value = Number(text);
  if (!/^[0-9]+$/.test(text) || value < range.min || value > range.max) {
    return fail(`${label}: un numero intero da ${range.min} a ${range.max}.`);
  }
  return ok(value);
}

/** Minuti di inattività e passi massimi, come testi dei campi. */
export function checkSession(idleRaw: string, stepsRaw: string): CheckResult<SessionInput> {
  const idle = wholeInRange(idleRaw, "Minuti di inattività", IDLE_MINUTES_RANGE);
  if (!idle.ok) return idle;
  const steps = wholeInRange(stepsRaw, "Passi massimi", MAX_STEPS_RANGE);
  if (!steps.ok) return steps;
  return ok({ idleMinutes: idle.value, maxSteps: steps.value });
}

// ---------------------------------------------------------------- segretari

/** I campi che l'utente modifica di un segretario. `channels` `null` = tutti i canali. */
export interface SecretaryDraft {
  name: string;
  systemPrompt: string;
  tools: readonly HubSecretaryTool[];
  channels: readonly string[] | null;
}

export const EMPTY_DRAFT: SecretaryDraft = { name: "", systemPrompt: "", tools: [], channels: null };

export function draftOf(secretary: HubSecretary): SecretaryDraft {
  return {
    name: secretary.name,
    systemPrompt: secretary.system_prompt,
    tools: secretary.tools,
    channels: secretary.channels.mode === "all" ? null : secretary.channels.channels,
  };
}

/** Aggiunge o toglie un elemento, mantenendo l'ordine della lista di riferimento. */
export function toggleIn<T>(list: readonly T[], item: T, order: readonly T[]): T[] {
  const next = list.includes(item) ? list.filter((x) => x !== item) : [...list, item];
  return order.filter((x) => next.includes(x));
}

/** Tool nell'ordine canonico. */
export function toggleTool(tools: readonly HubSecretaryTool[], tool: HubSecretaryTool): HubSecretaryTool[] {
  return toggleIn(
    tools,
    tool,
    SECRETARY_TOOLS.map((t) => t.id),
  );
}

/** Canali scelti nell'ordine dei canali esistenti (Generale in cima). */
export function toggleChannel(channels: readonly string[], channel: string, existing: readonly string[]): string[] {
  return toggleIn(channels, channel, existing);
}

/** Nome del segretario: minuscole, cifre e `-`, 1..24 caratteri, unico (come `normalize_name` del backend). */
export function checkSecretaryName(raw: string, taken: readonly string[]): CheckResult<string> {
  const name = raw.trim().toLowerCase();
  if (name === "" || charCount(name) > SECRETARY_NAME_MAX) {
    return fail(`Il nome deve avere da 1 a ${SECRETARY_NAME_MAX} caratteri.`);
  }
  if (!/^[a-z0-9-]+$/.test(name)) return fail("Il nome può contenere solo lettere minuscole, cifre e -.");
  if (name.startsWith("-") || name.endsWith("-")) return fail("Il nome non può iniziare o finire con -.");
  if (taken.some((t) => t.toLowerCase() === name)) return fail("Esiste già un segretario con questo nome.");
  return ok(name);
}

/** Bozza validata, con il nome normalizzato; `taken` sono i nomi degli altri segretari. */
export function checkDraft(draft: SecretaryDraft, taken: readonly string[]): CheckResult<SecretaryDraft> {
  const name = checkSecretaryName(draft.name, taken);
  if (!name.ok) return name;
  const prompt = draft.systemPrompt.trim();
  if (charCount(prompt) > SYSTEM_PROMPT_MAX) return fail(`Il system prompt supera i ${SYSTEM_PROMPT_MAX} caratteri.`);
  if (draft.channels !== null && draft.channels.length === 0) return fail("Scegli almeno un canale, oppure tutti.");
  return ok({ ...draft, name: name.value, systemPrompt: prompt });
}

/** Avviso mostrato al posto dell'interruttore finché l'integrazione è disabilitata (`HUB_AVAILABLE` nel backend). */
export const HUB_UNAVAILABLE_NOTICE =
  "L’integrazione dell’Agents HUB è momentaneamente disabilitata: era un’implementazione sperimentale.";

/** `false` solo se il backend dichiara l'HUB non disponibile; con le impostazioni non ancora lette non avvisa. */
export function hubAvailable(settings: { available: boolean } | null): boolean {
  return settings === null || settings.available;
}
