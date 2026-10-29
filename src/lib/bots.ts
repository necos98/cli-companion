// Bot: ogni sessione ha un nome preso da una pool configurabile e un avatar
// pixel-art deterministico. Funzioni pure a zero dipendenze, importabili sia
// dall'app sia dai test `node --test` (type-stripping). La pool predefinita e'
// in `bots.rs` (unica fonte): qui arriva con le impostazioni.

/** Lunghezza massima di un nome della pool (stesso limite di `bots.rs`). */
export const BOT_NAME_MAX = 24;
/** Margine per il suffisso numerico dei nomi a pool esaurita ("Bolt 12"). */
const SUFFIX_ROOM = 8;
/** Prefisso dei nomi quando la pool e' vuota. */
const FALLBACK_NAME = "Bot";

/** Confronto dei nomi senza distinguere le maiuscole (come in `bots.rs`). */
const nameKey = (name: string): string => name.toLowerCase();

/**
 * Nome per un nuovo bot: a caso tra quelli della pool non usati dai tab aperti
 * (`taken`). Pool esaurita: un nome della pool col primo suffisso numerico
 * libero ("Bolt 2"). Pool vuota: "Bot 1", "Bot 2"...
 */
export function pickBotName(
  pool: readonly string[],
  taken: Iterable<string>,
  random: () => number = Math.random,
): string {
  const used = new Set<string>();
  for (const name of taken) used.add(nameKey(name));
  const pickFrom = (list: readonly string[]) =>
    list[Math.min(list.length - 1, Math.floor(random() * list.length))];

  const free = pool.filter((n) => !used.has(nameKey(n)));
  if (free.length > 0) return pickFrom(free);

  const base = pool.length > 0 ? pickFrom(pool) : FALLBACK_NAME;
  // Senza pool si parte da "Bot 1"; con la pool esaurita il nome semplice e' gia' preso.
  for (let n = pool.length > 0 ? 2 : 1; ; n++) {
    const candidate = `${base} ${n}`;
    if (!used.has(nameKey(candidate))) return candidate;
  }
}

/**
 * Nome salvato nella sessione: senza caratteri di controllo, trimmato e
 * limitato in lunghezza. `undefined` se non resta nulla.
 */
export function sanitizeBotName(v: unknown, max = BOT_NAME_MAX + SUFFIX_ROOM): string | undefined {
  if (typeof v !== "string") return undefined;
  const name = v.replace(/[\u0000-\u001f\u007f-\u009f]/g, "").trim();
  if (name === "") return undefined;
  const limited = Array.from(name).slice(0, max).join("").trim();
  return limited === "" ? undefined : limited;
}

/**
 * Nomi dei tab ripristinati: chi ha un nome valido lo tiene, ma il secondo tab
 * con lo stesso nome (o chi non ne ha) ne riceve uno nuovo.
 */
export function assignRestoredNames(
  tabs: ReadonlyArray<{ tabId: string; botName?: string }>,
  pool: readonly string[],
  random: () => number = Math.random,
): Map<string, string> {
  const names = new Map<string, string>();
  const used = new Set<string>();
  const needNew: string[] = [];
  for (const t of tabs) {
    if (t.botName && !used.has(nameKey(t.botName))) {
      used.add(nameKey(t.botName));
      names.set(t.tabId, t.botName);
    } else {
      needNew.push(t.tabId);
    }
  }
  for (const id of needNew) {
    const name = pickBotName(pool, used, random);
    used.add(nameKey(name));
    names.set(id, name);
  }
  return names;
}

// ---------------------------------------------------------------------------
// Avatar
// ---------------------------------------------------------------------------

/** Colori del corpo (tonalita' distinte, leggibili sul tema scuro). */
export const BOT_PALETTE = [
  "#f87171",
  "#fb923c",
  "#fbbf24",
  "#a3e635",
  "#34d399",
  "#22d3ee",
  "#60a5fa",
  "#a78bfa",
  "#e879f9",
  "#fb7185",
] as const;

export const HEAD_SHAPES = 3;
export const ANTENNAS = 3;
export const EYE_STYLES = 3;
export const ACCESSORIES = 4;

export interface BotLook {
  /** Colore del corpo (da `BOT_PALETTE`). */
  color: string;
  /** 0 quadrata, 1 con angoli tagliati, 2 larga con orecchie. */
  head: number;
  /** 0 antenna con punta, 1 due antenne, 2 nessuna. */
  antenna: number;
  /** 0 occhi singoli, 1 occhi larghi, 2 visiera. */
  eyes: number;
  /** 0 nessuno, 1 bocca, 2 guance, 3 bulloni. */
  accessory: number;
}

/** FNV-1a a 32 bit: stabile tra piattaforme e versioni. */
function hash(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/** Aspetto dell'avatar, deterministico: stesso nome = stesso bot. */
export function botLook(name: string): BotLook {
  let h = hash(name.trim().toLowerCase());
  const take = (n: number): number => {
    const v = h % n;
    h = Math.floor(h / n);
    return v;
  };
  const color = BOT_PALETTE[take(BOT_PALETTE.length)];
  return {
    color,
    head: take(HEAD_SHAPES),
    antenna: take(ANTENNAS),
    eyes: take(EYE_STYLES),
    accessory: take(ACCESSORIES),
  };
}

// ---------------------------------------------------------------------------
// Disegno dell'avatar (griglia di pixel)
// ---------------------------------------------------------------------------

/** Lato della griglia dell'avatar (viewBox dell'SVG). */
export const BOT_GRID = 12;

/** `body` = colore del bot, `dark` = occhi e bocca, `light` = riflessi. */
export type PixelTone = "body" | "dark" | "light";

export interface PixelRect {
  x: number;
  y: number;
  w: number;
  h: number;
  tone: PixelTone;
}

const px = (x: number, y: number, w: number, h: number, tone: PixelTone): PixelRect => ({
  x,
  y,
  w,
  h,
  tone,
});

const HEAD_RECTS: readonly (readonly PixelRect[])[] = [
  [px(2, 3, 8, 7, "body")],
  [px(3, 3, 6, 1, "body"), px(2, 4, 8, 5, "body"), px(3, 9, 6, 1, "body")],
  [px(3, 3, 6, 7, "body"), px(1, 5, 2, 3, "body"), px(9, 5, 2, 3, "body")],
];

const ANTENNA_RECTS: readonly (readonly PixelRect[])[] = [
  [px(5, 1, 2, 2, "body")],
  [px(3, 1, 1, 2, "body"), px(8, 1, 1, 2, "body")],
  [],
];

const EYE_RECTS: readonly (readonly PixelRect[])[] = [
  [px(4, 5, 1, 2, "dark"), px(7, 5, 1, 2, "dark")],
  [px(3, 5, 2, 2, "dark"), px(7, 5, 2, 2, "dark")],
  [px(3, 5, 6, 2, "dark"), px(4, 5, 1, 1, "light")],
];

const ACCESSORY_RECTS: readonly (readonly PixelRect[])[] = [
  [],
  [px(5, 8, 2, 1, "dark")],
  [px(3, 7, 1, 1, "light"), px(8, 7, 1, 1, "light")],
  [px(3, 3, 1, 1, "light"), px(8, 3, 1, 1, "light")],
];

/** Rettangoli dell'avatar sulla griglia `BOT_GRID`, dal fondo verso l'alto. */
export function botPixels(look: BotLook): PixelRect[] {
  return [
    ...HEAD_RECTS[look.head],
    ...ANTENNA_RECTS[look.antenna],
    ...EYE_RECTS[look.eyes],
    ...ACCESSORY_RECTS[look.accessory],
  ];
}

// ---------------------------------------------------------------------------
// Immagini dei nomi della pool
// ---------------------------------------------------------------------------

/** Immagine di un nome della pool: PNG pixelato (base64) e colore dominante. */
export interface BotImage {
  png: string;
  color: string;
}

export interface BotImageSource {
  names: readonly string[];
  images?: Readonly<Record<string, BotImage>>;
}

/** Base64 rigoroso: niente caratteri che possano uscire da un URL `data:`. */
const BASE64_PATTERN = /^[A-Za-z0-9+/]+={0,2}$/;
/** Colore `#rrggbb`. */
const HEX_COLOR_PATTERN = /^#[0-9a-f]{6}$/i;
/** Nome con suffisso numerico generato da `pickBotName` ("Bolt 2"). */
const SUFFIXED_NAME_PATTERN = /^(.+) (\d+)$/;

/** Immagine ben formata, altrimenti `undefined` (difesa in profondita'). */
function validImage(image: BotImage | undefined): BotImage | undefined {
  if (!image || typeof image.png !== "string" || typeof image.color !== "string") return undefined;
  return BASE64_PATTERN.test(image.png) && HEX_COLOR_PATTERN.test(image.color) ? image : undefined;
}

/** Ortografia del nome nella pool (senza badare alle maiuscole). */
function poolSpelling(names: readonly string[], name: string): string | undefined {
  const key = nameKey(name);
  return names.find((n) => nameKey(n) === key);
}

/**
 * Immagine del bot `botName`: il nome esatto della pool vince; altrimenti un
 * nome con suffisso numerico ("Bolt 2") usa l'immagine del nome base.
 */
export function botImageOf(settings: BotImageSource, botName: string): BotImage | undefined {
  const images = settings.images;
  if (!images) return undefined;
  const exact = poolSpelling(settings.names, botName);
  if (exact !== undefined) return validImage(images[exact]);
  const match = SUFFIXED_NAME_PATTERN.exec(botName);
  if (!match) return undefined;
  const base = poolSpelling(settings.names, match[1]);
  return base === undefined ? undefined : validImage(images[base]);
}

/** URL `data:` dell'immagine (solo per immagini gia' validate). */
export function botImageHref(image: BotImage): string {
  return `data:image/png;base64,${image.png}`;
}
