import type { Rect, Size } from "../../lib/botImage";
import type { BotPose } from "../../lib/office";

// Corpo del bot sulla griglia dello sprite (16 x 24 pixel = 2 x 3 unita'): la
// testa e' l'avatar 12 x 12 di `BotAvatar`, centrata in alto. Lo sprite guarda a
// destra; guardare a sinistra e' uno specchio fatto dal componente.

/** Dimensioni dello sprite (pixel). */
export const SPRITE_W = 16;
export const SPRITE_H = 24;
/** Scostamento orizzontale della testa (pixel). */
export const HEAD_X = 2;
/** Di quanto scende il busto quando il bot e' seduto (pixel). */
export const SIT_DROP = 2;
/** Di quanto cala la testa quando il bot e' a pezzi (pixel). */
export const SLUMP_DROP = 2;

/** `shade` e' il colore del bot scurito (braccia e gambe); gli altri sono fissi. */
export type PartTone = "body" | "shade" | "dark" | "light" | "mug" | "steam" | "clipboard" | "paper";

export interface Part {
  x: number;
  y: number;
  w: number;
  h: number;
  tone: PartTone;
}

const p = (x: number, y: number, w: number, h: number, tone: PartTone): Part => ({ x, y, w, h, tone });

/** Posa dello sprite: quelle del bot piu' il cammino. */
export type SpritePose = BotPose | "walking";

/** Pose in cui il bot e' seduto (sedia o divano). */
const SEATED: ReadonlySet<SpritePose> = new Set<SpritePose>(["typing", "slumped", "sitting"]);
export const isSeated = (pose: SpritePose): boolean => SEATED.has(pose);

export const TORSO: readonly Part[] = [
  p(4, 10, 8, 7, "body"),
  p(6, 12, 4, 2, "light"),
  p(4, 16, 8, 1, "dark"),
];

/** Gambe in piedi: sinistra e destra animate a parte (passo, battito del piede). */
export const LEG_LEFT: readonly Part[] = [p(5, 17, 2, 5, "shade"), p(5, 22, 3, 2, "dark")];
export const LEG_RIGHT: readonly Part[] = [p(9, 17, 2, 5, "shade"), p(9, 22, 3, 2, "dark")];

/** Gambe da seduto: coscia in avanti, stinco in basso, scarpa. */
export const LEGS_SEATED: readonly Part[] = [
  p(5, 19, 8, 2, "shade"),
  p(11, 21, 2, 2, "shade"),
  p(11, 23, 4, 1, "dark"),
];

export interface ArmParts {
  left: readonly Part[];
  right: readonly Part[];
}

const HANG_LEFT = [p(2, 11, 2, 4, "shade"), p(2, 15, 2, 1, "dark")] as const;
const HANG_RIGHT = [p(12, 11, 2, 4, "shade"), p(12, 15, 2, 1, "dark")] as const;
const UP_LEFT = [p(2, 11, 2, 2, "shade"), p(1, 4, 2, 8, "shade"), p(1, 3, 2, 1, "dark")] as const;
const UP_RIGHT = [p(12, 11, 2, 2, "shade"), p(13, 4, 2, 8, "shade"), p(13, 3, 2, 1, "dark")] as const;

const ARMS: Record<SpritePose, ArmParts> = {
  walking: { left: HANG_LEFT, right: HANG_RIGHT },
  standing: { left: HANG_LEFT, right: HANG_RIGHT },
  looking: { left: HANG_LEFT, right: HANG_RIGHT },
  sitting: { left: HANG_LEFT, right: HANG_RIGHT },
  slumped: { left: HANG_LEFT, right: HANG_RIGHT },
  // Avambracci in avanti sulla tastiera.
  typing: {
    left: [p(9, 14, 5, 2, "shade"), p(14, 14, 1, 2, "dark")],
    right: [p(11, 12, 5, 2, "shade"), p(16, 12, 1, 2, "dark")],
  },
  "raising-hand": { left: HANG_LEFT, right: UP_RIGHT },
  celebrating: { left: UP_LEFT, right: UP_RIGHT },
  // Braccia incrociate sul petto.
  waiting: {
    left: [p(3, 12, 10, 3, "shade"), p(10, 12, 2, 1, "dark")],
    right: [],
  },
  // Gesticola mentre parla.
  chatting: {
    left: HANG_LEFT,
    right: [p(12, 11, 3, 2, "shade"), p(14, 7, 2, 5, "shade"), p(14, 6, 2, 1, "dark")],
  },
  // Tazza in mano, alzata verso il viso.
  coffee: {
    left: HANG_LEFT,
    right: [
      p(12, 11, 3, 2, "shade"),
      p(14, 8, 2, 4, "shade"),
      p(14, 7, 2, 1, "dark"),
      p(13, 4, 3, 3, "mug"),
      p(16, 5, 1, 2, "mug"),
    ],
  },
};

export const armsOf = (pose: SpritePose): ArmParts => ARMS[pose];

// ---------------------------------------------------------------- corpo da immagine

/** Zona del corpo (busto + testa) che l'immagine di un bot sostituisce. */
export const BODY_ZONE: Rect = { x: 2, y: 0, w: 12, h: 17 };
/** Spalle del busto: da qui partono le braccia, e quanto in alto comincia. */
const TORSO_LEFT = 4;
const TORSO_RIGHT = 12;
const TORSO_TOP = 10;
/** Di quanto al massimo si abbassano le braccia per restare attaccate a un corpo basso. */
export const MAX_ARM_DROP = 3;

/**
 * Riquadro dell'immagine nella zona del corpo: scala uniforme che la fa entrare
 * (un'immagine 24 x 34 e' 12 x 17 pixel dello sprite), appoggiata sulle gambe
 * (in basso) e centrata in orizzontale.
 */
export function imageBodyBox(size: Size): Rect {
  const scale = Math.min(BODY_ZONE.w / size.w, BODY_ZONE.h / size.h);
  const w = size.w * scale;
  const h = size.h * scale;
  return {
    x: BODY_ZONE.x + (BODY_ZONE.w - w) / 2,
    y: BODY_ZONE.y + BODY_ZONE.h - h,
    w,
    h,
  };
}

export interface ArmShift {
  dx: number;
  dy: number;
}

/**
 * Spostamento delle braccia per restare attaccate al corpo-immagine: verso il
 * centro se l'immagine e' piu' stretta del busto, in basso (fino a
 * `MAX_ARM_DROP`) se e' piu' bassa. Interi, per restare sulla griglia.
 */
export function armShiftOf(box: Rect): { left: ArmShift; right: ArmShift } {
  const dy = Math.round(Math.min(MAX_ARM_DROP, Math.max(0, box.y - TORSO_TOP)));
  return {
    left: { dx: Math.round(Math.max(0, box.x - TORSO_LEFT)), dy },
    right: { dx: -Math.round(Math.max(0, TORSO_RIGHT - (box.x + box.w))), dy },
  };
}

/** Vapore sopra la tazza (solo pausa caffe'). */
export const STEAM: readonly Part[] = [p(13, 2, 1, 1, "steam"), p(15, 1, 1, 1, "steam")];

/** Cartellina del bot Task Intake: la tiene davanti, sul lato sinistro dello sprite. */
export const CLIPBOARD: readonly Part[] = [
  p(0, 12, 4, 6, "clipboard"),
  p(1, 13, 2, 4, "paper"),
  p(1, 12, 2, 1, "dark"),
];

/** Fattore con cui si scurisce il colore del bot per braccia e gambe. */
const SHADE_FACTOR = 0.62;
const HEX_RADIX = 16;
const BYTE = 0xff;

/** Versione scurita di un colore `#rrggbb`. */
export function shadeOf(hex: string): string {
  const n = parseInt(hex.slice(1), HEX_RADIX);
  const channel = (shift: number) => Math.round(((n >> shift) & BYTE) * SHADE_FACTOR);
  return `rgb(${channel(16)}, ${channel(8)}, ${channel(0)})`;
}
