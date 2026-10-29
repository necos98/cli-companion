// Ufficio: geometria delle stanze. Ogni workspace ha una stanza con parete di
// fondo, zona relax (caffe', divano, finestra), server, corridoio per le
// chiacchiere e file di scrivanie. Funzioni pure a zero dipendenze: tutte le
// coordinate sono in "unita' stanza" (u) e un punto indica i PIEDI del bot
// (centro in basso). Importabile sia dall'app sia dai test `node --test`.

// ---------------------------------------------------------------- tipi

/** Larghezza della stanza (u). */
export const ROOM_WIDTH = 24;
/** Larghezza massima del nome sotto un bot (u): oltre, il nome si tronca. */
export const BOT_NAME_MAX_WIDTH = 2.4;
/** Dimensioni dello sprite di un bot (u). */
export const BOT_WIDTH = 2;
export const BOT_HEIGHT = 3;

/** 1 = guarda a destra, -1 = a sinistra. */
export type Facing = 1 | -1;
export interface Point {
  x: number;
  y: number;
}
export type SpotKind = "coffee" | "sofa" | "window" | "plant" | "floor";
export interface Spot extends Point {
  id: string;
  kind: SpotKind;
  facing: Facing;
}
export interface ChatArea {
  id: string;
  /** 2 posti uno di fronte all'altro (0 guarda a destra, 1 a sinistra) + un terzo. */
  seats: readonly [Spot, Spot, Spot];
}
export type FurnitureKind = "board" | "window" | "server" | "coffee" | "plant" | "sofa";
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Furniture extends Rect {
  kind: FurnitureKind;
}
/** La scrivania (angolo in alto a sinistra + misure); il monitor sta sulla meta' sinistra. */
export interface Desk extends Rect {
  /** Sedia: dove siede un bot al lavoro. */
  seat: Point;
  /** Accanto alla scrivania: attenzione, festa, idle senza meta. */
  standing: Point;
  /** Il bot alla scrivania guarda il monitor. */
  facing: Facing;
}
export interface RoomLayout {
  width: number;
  height: number;
  furniture: Furniture[];
  /** Una per ogni scrivania della stanza. */
  desks: Desk[];
  /** Spot per l'idle (caffe', divano, finestra, pianta, pavimento): sempre >= desks.length. */
  spots: Spot[];
  /** Almeno una. */
  chatAreas: ChatArea[];
  /** Attesa in background davanti al rack dei server. */
  server: Spot[];
}

// ---------------------------------------------------------------- costanti

/** Scrivanie per ogni fila della stanza. */
export const DESKS_PER_ROW = 4;
/** Y della prima fila di scrivanie e passo tra le file. */
export const DESK_AREA_Y = 12;
const ROW_HEIGHT = 5.5;
const BOTTOM_MARGIN = 0.5;
/** Larghezza di una colonna di scrivanie e misure della scrivania. */
const COLUMN_WIDTH = 6;
const DESK_OFFSET_X = 0.8;
const DESK_W = 3.2;
const DESK_H = 2.6;
/** Sedia e punto "in piedi" rispetto all'angolo della colonna / della fila. */
const SEAT_DX = 4.8;
const STANDING_DX = 5.6;
const STANDING_DY = 3.6;
/** Corridoio sotto ogni fila: spot a pavimento. */
const FLOOR_AISLE_DY = 4.6;
const FLOOR_OFFSET_X = 1.5;
/** Corridoio delle chiacchiere sopra le scrivanie: due posti affiancati e un terzo dietro. */
const CHAT_HALF_GAP = 1.4;
const CHAT_BACK_DY = 0.9;
const CHAT_ROW_SEATS_Y = 10.4;
const LOUNGE_CHAT_CENTERS = [6, 18] as const;
const DESK_CHAT_CENTER = 16.5;
const DESK_CHAT_DY = 0.5;

const spot = (id: string, kind: SpotKind, x: number, y: number, facing: Facing): Spot => ({
  id,
  kind,
  x,
  y,
  facing,
});

// ---------------------------------------------------------------- parti fisse

function fixedFurniture(): Furniture[] {
  return [
    { kind: "board", x: 1, y: 0.8, w: 7, h: 3 },
    { kind: "window", x: 10, y: 0.8, w: 6, h: 3.2 },
    { kind: "server", x: 20.5, y: 1.5, w: 2.5, h: 4.5 },
    { kind: "coffee", x: 0.5, y: 4.2, w: 3, h: 2.8 },
    { kind: "plant", x: 8.6, y: 4.5, w: 1.4, h: 2.5 },
    { kind: "sofa", x: 10, y: 5.6, w: 7, h: 2 },
  ];
}

function loungeSpots(): Spot[] {
  return [
    spot("coffee-0", "coffee", 4, 7.6, -1),
    spot("coffee-1", "coffee", 5.4, 8.2, -1),
    spot("plant-0", "plant", 7.4, 7.8, 1),
    spot("sofa-0", "sofa", 11.2, 7.3, 1),
    spot("sofa-1", "sofa", 13.6, 7.3, 1),
    spot("sofa-2", "sofa", 16, 7.3, -1),
    spot("window-0", "window", 17.4, 6.6, -1),
    spot("window-1", "window", 18.8, 7.8, -1),
  ];
}

function serverSpots(): Spot[] {
  return [
    spot("server-0", "floor", 21, 7.6, 1),
    spot("server-1", "floor", 22.6, 7.6, -1),
    spot("server-2", "floor", 21.8, 8.6, 1),
  ];
}

/** Area chiacchiere centrata in `cx`: i primi due posti a `y`, il terzo piu' in alto. */
function chatArea(id: string, cx: number, y: number): ChatArea {
  return {
    id,
    seats: [
      spot(`${id}-0`, "floor", cx - CHAT_HALF_GAP, y, 1),
      spot(`${id}-1`, "floor", cx + CHAT_HALF_GAP, y, -1),
      spot(`${id}-2`, "floor", cx, y - CHAT_BACK_DY, 1),
    ],
  };
}

// ---------------------------------------------------------------- scrivanie

/** Angolo in alto della fila `r`. */
const rowTop = (r: number): number => DESK_AREA_Y + r * ROW_HEIGHT;

function deskAt(i: number): Desk {
  const col = i % DESKS_PER_ROW;
  const y0 = rowTop(Math.floor(i / DESKS_PER_ROW));
  const x0 = col * COLUMN_WIDTH;
  return {
    x: x0 + DESK_OFFSET_X,
    y: y0,
    w: DESK_W,
    h: DESK_H,
    seat: { x: x0 + SEAT_DX, y: y0 + DESK_H },
    standing: { x: x0 + STANDING_DX, y: y0 + STANDING_DY },
    facing: -1,
  };
}

function floorSpotAt(i: number): Spot {
  const col = i % DESKS_PER_ROW;
  const y = rowTop(Math.floor(i / DESKS_PER_ROW)) + FLOOR_AISLE_DY;
  return spot(`floor-${i}`, "floor", col * COLUMN_WIDTH + FLOOR_OFFSET_X, y, col % 2 === 0 ? 1 : -1);
}

const range = (n: number): number[] => Array.from({ length: n }, (_, i) => i);

/** Aree chiacchiere: due fisse nella zona relax, una per ogni fila oltre la prima. */
function chatAreas(rows: number): ChatArea[] {
  const lounge = LOUNGE_CHAT_CENTERS.map((cx, i) => chatArea(`chat-${i}`, cx, CHAT_ROW_SEATS_Y));
  const aisles = range(rows - 1).map((k) =>
    chatArea(`chat-${k + 2}`, DESK_CHAT_CENTER, rowTop(k + 1) - DESK_CHAT_DY),
  );
  return [...lounge, ...aisles];
}

/** Stanza con `deskCount` scrivanie (le righe sono complete, minimo una). Deterministica. */
export function roomLayout(deskCount: number): RoomLayout {
  const count = Math.max(1, Math.floor(deskCount));
  const rows = Math.ceil(count / DESKS_PER_ROW);
  return {
    width: ROOM_WIDTH,
    height: DESK_AREA_Y + rows * ROW_HEIGHT + BOTTOM_MARGIN,
    furniture: fixedFurniture(),
    desks: range(count).map(deskAt),
    spots: [...loungeSpots(), ...range(count).map(floorSpotAt)],
    chatAreas: chatAreas(rows),
    server: serverSpots(),
  };
}

/** Spot o posto di una chiacchierata con quell'id, se esiste nella stanza. */
export function findSpot(layout: RoomLayout, id: string): Spot | undefined {
  return (
    layout.spots.find((s) => s.id === id) ??
    layout.chatAreas.flatMap((a) => a.seats).find((s) => s.id === id)
  );
}
