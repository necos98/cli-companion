// Ufficio: una stanza per workspace, un bot per ogni tab aperto. Qui la logica
// pura: attivita' dei bot, scrivanie stabili, stanze, pianificazione dell'idle
// (giri per la stanza e chiacchiere). La geometria sta in `officeLayout.ts` e
// posizione/posa di ogni bot in `officePlacement.ts`, entrambe riesportate da
// qui. Funzioni pure a zero dipendenze: orologio e casualita' sono iniettati.
// Importabile sia dall'app sia dai test `node --test`.

import type { AgentPhase } from "./agents.ts";
import { isIntakeLaunch } from "./automation.ts";
import { DESKS_PER_ROW, findSpot } from "./officeLayout.ts";
import type { ChatArea, RoomLayout, Spot, SpotKind } from "./officeLayout.ts";

export * from "./officeLayout.ts";
export * from "./officePlacement.ts";

// ---------------------------------------------------------------- attivita'

export type BotActivity = "working" | "idle" | "attention" | "background" | "completed" | "failed";

export interface ActivityInput {
  agent?: { phase: AgentPhase; ended?: boolean };
  /** Euristica "shell occupata" (busyByKey). */
  busy: boolean;
  /** Stato del worker (`WorkerStatus` di automation.ts, stringa aperta). */
  automation?: { status: string };
}

/** Attivita' dei worker noti; `closed` e stati sconosciuti ricadono sul resto. */
const WORKER_ACTIVITY: Readonly<Record<string, BotActivity>> = {
  running: "working",
  starting: "working",
  needs_input: "attention",
  idle: "attention",
  background: "background",
  completed: "completed",
  failed: "failed",
};

const AGENT_ACTIVITY: Readonly<Record<AgentPhase, BotActivity>> = {
  running: "working",
  "needs-input": "attention",
  idle: "idle",
  finished: "idle",
};

/**
 * Cosa sta facendo il bot di un tab. Lo stato del worker vince quando e' noto;
 * poi l'agente (se non terminato); altrimenti conta solo la shell occupata.
 */
export function botActivity(tab: ActivityInput): BotActivity {
  const status = tab.automation?.status;
  const known = status !== undefined && Object.prototype.hasOwnProperty.call(WORKER_ACTIVITY, status);
  const fromWorker = known ? WORKER_ACTIVITY[status] : undefined;
  if (fromWorker) return fromWorker;
  if (tab.agent && !tab.agent.ended) return AGENT_ACTIVITY[tab.agent.phase];
  return tab.busy ? "working" : "idle";
}

/** Stato in italiano per il tooltip. */
export const ACTIVITY_LABELS: Record<BotActivity, string> = {
  working: "Al lavoro",
  idle: "In pausa",
  attention: "Richiede attenzione",
  background: "In attesa in background",
  completed: "Ha consegnato",
  failed: "Fallito",
};

// ---------------------------------------------------------------- sessioni

/** Un bot dell'ufficio (costruito da App per ogni tab aperto). */
export interface OfficeSession {
  /** Chiave del tab. */
  key: string;
  bot: string;
  workspaceId: string;
  workspaceName: string;
  /** `tabLabel(t)` (titolo del task per i worker). */
  label: string;
  activity: BotActivity;
  /** Tab aperto dal preset Task Intake. */
  intake: boolean;
  /** Titolo del task: solo per i tab dei worker. */
  taskTitle?: string;
}

/** Struttura minima di un tab di App, per costruire la sessione. */
export interface OfficeTabLike {
  key: string;
  bot: string;
  workspace: { id: string; name: string };
  launchId?: string;
  agent?: { phase: AgentPhase; ended?: boolean };
  automation?: { status: string; title: string };
}

/** Sessione dell'ufficio di un tab, con la shell occupata e l'etichetta gia' calcolate. */
export function officeSession(tab: OfficeTabLike, busy: boolean, label: string): OfficeSession {
  return {
    key: tab.key,
    bot: tab.bot,
    workspaceId: tab.workspace.id,
    workspaceName: tab.workspace.name,
    label,
    activity: botActivity({ agent: tab.agent, busy, automation: tab.automation }),
    intake: isIntakeLaunch(tab.launchId),
    ...(tab.automation ? { taskTitle: tab.automation.title } : {}),
  };
}

// ---------------------------------------------------------------- scrivanie e stanze

export const MIN_DESKS = 4;

/** Scrivania (indice nella stanza del suo workspace) di ogni bot, per chiave del tab. */
export type DeskMap = ReadonlyMap<string, number>;

type DeskSession = Pick<OfficeSession, "key" | "workspaceId">;

/** Indice piu' basso non presente in `used`. */
function lowestFree(used: ReadonlySet<number>): number {
  let i = 0;
  while (used.has(i)) i++;
  return i;
}

/**
 * Scrivanie stabili: chi c'era tiene la sua, i tab chiusi la liberano, i nuovi
 * (nell'ordine dato) prendono la piu' bassa libera della loro stanza.
 * Nulla cambiato -> stessa istanza di `prev`.
 */
export function assignDesks(prev: DeskMap, sessions: ReadonlyArray<DeskSession>): DeskMap {
  const next = new Map<string, number>();
  const usedByRoom = new Map<string, Set<number>>();
  const usedIn = (workspaceId: string): Set<number> => {
    const used = usedByRoom.get(workspaceId) ?? new Set<number>();
    usedByRoom.set(workspaceId, used);
    return used;
  };
  const occupy = (s: DeskSession, desk: number) => {
    next.set(s.key, desk);
    usedIn(s.workspaceId).add(desk);
  };

  for (const s of sessions) {
    const desk = prev.get(s.key);
    if (desk !== undefined && !next.has(s.key)) occupy(s, desk);
  }
  const kept = next.size;
  for (const s of sessions) {
    if (!next.has(s.key)) occupy(s, lowestFree(usedIn(s.workspaceId)));
  }
  return kept === prev.size && next.size === kept ? prev : next;
}

export interface OfficeRoomBot extends OfficeSession {
  desk: number;
}
export interface OfficeRoom {
  workspaceId: string;
  name: string;
  /** max(MIN_DESKS, scrivania piu' alta + 1), arrotondato a multiplo di DESKS_PER_ROW. */
  deskCount: number;
  /** Ordinati per scrivania. */
  bots: OfficeRoomBot[];
}

function deskCountFor(bots: readonly OfficeRoomBot[]): number {
  const needed = Math.max(MIN_DESKS, ...bots.map((b) => b.desk + 1));
  return Math.ceil(needed / DESKS_PER_ROW) * DESKS_PER_ROW;
}

/** Sessioni con scrivania raggruppate per workspace, nell'ordine di comparsa. */
function groupByWorkspace(
  sessions: readonly OfficeSession[],
  desks: DeskMap,
): Map<string, OfficeRoomBot[]> {
  const groups = new Map<string, OfficeRoomBot[]>();
  for (const s of sessions) {
    const desk = desks.get(s.key);
    if (desk === undefined) continue;
    const list = groups.get(s.workspaceId) ?? [];
    list.push({ ...s, desk });
    groups.set(s.workspaceId, list);
  }
  return groups;
}

/**
 * Una stanza per workspace con almeno un tab, nell'ordine della Sidebar
 * (`workspaceOrder`); workspace assenti dall'ordine in coda, nell'ordine di
 * comparsa. Sessioni senza scrivania in `desks`: ignorate.
 */
export function officeRooms(
  sessions: readonly OfficeSession[],
  workspaceOrder: readonly string[],
  desks: DeskMap,
): OfficeRoom[] {
  const groups = groupByWorkspace(sessions, desks);
  const ids = [
    ...workspaceOrder.filter((id) => groups.has(id)),
    ...[...groups.keys()].filter((id) => !workspaceOrder.includes(id)),
  ];
  return ids.map((id) => {
    const bots = groups.get(id)!.sort((a, b) => a.desk - b.desk);
    const first = sessions.find((s) => s.workspaceId === id)!;
    return { workspaceId: id, name: first.workspaceName, deskCount: deskCountFor(bots), bots };
  });
}

// ---------------------------------------------------------------- pianificazione idle

export const IDLE_MIN_MS = 4000;
export const IDLE_MAX_MS = 10000;
/** Probabilita' che un bot libero inizi una chiacchierata (se ha un compagno e un'area libera). */
export const CHAT_CHANCE = 0.35;
/** Probabilita' che un bot libero scelga un punto a pavimento invece di un punto d'interesse. */
export const FLOOR_CHANCE = 0.2;
/** Probabilita' che una chiacchierata sia in tre (con almeno due compagni liberi). */
const TRIO_CHANCE = 0.5;
const PAIR = 2;
const TRIO = 3;

export interface IdleGoal {
  /** Id di uno spot di `layout.spots` o di un posto di una `ChatArea`. */
  spotId: string;
  kind: SpotKind | "chat";
  /** Fine della meta (ms, stesso orologio di `now`). */
  until: number;
  /** Id della ChatArea quando chiacchiera. */
  chat?: string;
}
/** Meta di ogni bot in idle, per chiave del tab. */
export type IdleGoals = ReadonlyMap<string, IdleGoal>;

export interface IdlePlanInput {
  layout: RoomLayout;
  /** Chiavi dei bot in idle della stanza, in ordine stabile (scrivania). */
  idle: readonly string[];
  goals: IdleGoals;
  now: number;
  /** Valori in [0,1). */
  random: () => number;
}

/** Indice casuale valido in una lista non vuota. */
const randomIndex = (length: number, random: () => number): number =>
  Math.min(length - 1, Math.floor(random() * length));

/** Durata di una meta in ms, tra IDLE_MIN_MS e IDLE_MAX_MS inclusi. */
function idleDuration(random: () => number): number {
  return IDLE_MIN_MS + Math.floor(random() * (IDLE_MAX_MS - IDLE_MIN_MS + 1));
}

/** Toglie e restituisce un elemento casuale della lista. */
function takeRandom<T>(list: T[], random: () => number): T {
  return list.splice(randomIndex(list.length, random), 1)[0];
}

/** Mete da tenere: bot ancora in idle, non scadute, su uno spot esistente; i gruppi con meno di 2 membri si sciolgono. */
function keepGoals(input: IdlePlanInput): Map<string, IdleGoal> {
  const { layout, idle, goals, now } = input;
  const stillIdle = new Set(idle);
  const kept = new Map<string, IdleGoal>();
  const taken = new Set<string>();
  for (const [key, goal] of goals) {
    const valid = stillIdle.has(key) && goal.until > now && findSpot(layout, goal.spotId);
    if (!valid || taken.has(goal.spotId)) continue;
    taken.add(goal.spotId);
    kept.set(key, goal);
  }
  const members = new Map<string, string[]>();
  for (const [key, goal] of kept) {
    if (goal.chat) members.set(goal.chat, [...(members.get(goal.chat) ?? []), key]);
  }
  for (const group of members.values()) {
    if (group.length < PAIR) group.forEach((key) => kept.delete(key));
  }
  return kept;
}

/** Cosa e' ancora libero durante una passata di pianificazione. */
interface Free {
  bots: string[];
  spots: Spot[];
  areas: ChatArea[];
}

function freeResources(
  layout: RoomLayout,
  idle: readonly string[],
  kept: ReadonlyMap<string, IdleGoal>,
): Free {
  const occupied = new Set<string>();
  for (const goal of kept.values()) {
    occupied.add(goal.spotId);
    if (goal.chat) occupied.add(goal.chat);
  }
  return {
    bots: idle.filter((key) => !kept.has(key)),
    spots: layout.spots.filter((s) => !occupied.has(s.id)),
    areas: layout.chatAreas.filter(
      (a) => !occupied.has(a.id) && !a.seats.some((s) => occupied.has(s.id)),
    ),
  };
}

/** Prova a formare una chiacchierata col bot (gia' tolto da `free.bots`); restituisce le mete dei membri. */
function formChat(
  bot: string,
  free: Free,
  now: number,
  random: () => number,
): Map<string, IdleGoal> | null {
  if (free.bots.length === 0 || free.areas.length === 0 || random() >= CHAT_CHANCE) return null;
  const size = free.bots.length >= PAIR && random() < TRIO_CHANCE ? TRIO : PAIR;
  const area = takeRandom(free.areas, random);
  const members = [bot];
  while (members.length < size) members.push(takeRandom(free.bots, random));
  const until = now + idleDuration(random);
  const goals = new Map<string, IdleGoal>();
  members.forEach((key, i) => {
    goals.set(key, { spotId: area.seats[i].id, kind: "chat", until, chat: area.id });
  });
  return goals;
}

/** Spot libero per il bot: punto d'interesse, o pavimento (a caso o se non ci sono altri), evitando quello appena lasciato. */
function pickSpot(free: Free, previous: string | undefined, random: () => number): Spot | null {
  const poi = free.spots.filter((s) => s.kind !== "floor");
  const floor = free.spots.filter((s) => s.kind === "floor");
  let pool = poi;
  if (poi.length === 0) pool = floor;
  else if (floor.length > 0 && random() < FLOOR_CHANCE) pool = floor;
  const elsewhere = (list: Spot[]) => list.filter((s) => s.id !== previous);
  const candidates = [elsewhere(pool), elsewhere(free.spots), pool].find((l) => l.length > 0);
  if (!candidates) return null;
  return candidates[randomIndex(candidates.length, random)];
}

/** Meta a uno spot singolo; lo toglie dagli spot liberi. */
function soloGoal(
  free: Free,
  previous: IdleGoal | undefined,
  now: number,
  random: () => number,
): IdleGoal | null {
  const spot = pickSpot(free, previous?.spotId, random);
  if (!spot) return null;
  free.spots = free.spots.filter((s) => s.id !== spot.id);
  return { spotId: spot.id, kind: spot.kind, until: now + idleDuration(random) };
}

/**
 * Pianifica le mete dei bot in idle di UNA stanza: tiene quelle ancora valide,
 * scioglie i gruppi rimasti in meno di 2 e assegna spot o chiacchierate ai
 * bot liberi. Mai due bot sullo stesso spot. Nulla cambiato -> stessa istanza
 * di `goals`.
 */
export function planIdle(input: IdlePlanInput): IdleGoals {
  const { layout, idle, goals, now, random } = input;
  const result = keepGoals(input);
  const keptCount = result.size;
  const free = freeResources(layout, idle, result);

  while (free.bots.length > 0) {
    const bot = free.bots.shift()!;
    const chat = formChat(bot, free, now, random);
    if (chat) {
      for (const [key, goal] of chat) result.set(key, goal);
      continue;
    }
    const goal = soloGoal(free, goals.get(bot), now, random);
    if (goal) result.set(bot, goal);
  }

  const unchanged = result.size === keptCount && keptCount === goals.size;
  return unchanged ? goals : result;
}

/** Ordine di preferenza dei posti a riposo: prima il divano, poi il caffe', poi il resto. */
const RESTING_ORDER: readonly SpotKind[] = ["sofa", "coffee"];

const restingRank = (kind: SpotKind): number => {
  const i = RESTING_ORDER.indexOf(kind);
  return i === -1 ? RESTING_ORDER.length : i;
};

/** prefers-reduced-motion: un posto fisso per ogni bot in idle (prima divano, poi caffe', poi gli altri), until = Infinity. */
export function restingGoals(layout: RoomLayout, idle: readonly string[]): IdleGoals {
  // sort e' stabile: a pari rango resta l'ordine del layout.
  const spots = [...layout.spots].sort((a, b) => restingRank(a.kind) - restingRank(b.kind));
  const goals = new Map<string, IdleGoal>();
  idle.forEach((key, i) => {
    const spot = spots[i];
    if (spot) goals.set(key, { spotId: spot.id, kind: spot.kind, until: Infinity });
  });
  return goals;
}
