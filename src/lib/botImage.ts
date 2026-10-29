// Immagine di un bot: l'utente associa un'immagine a un nome della pool e
// l'app la trasforma in pixel-art. Pipeline (tutta pura, a zero dipendenze,
// testabile senza canvas; decodifica ed esportazione stanno nello strato canvas):
//   1. `opaqueBounds` + `cropImage`: via i margini trasparenti;
//   2. `fitSize` + `resample`: riduzione (o ingrandimento) a media d'area
//      dentro BOT_IMAGE_MAX_W x BOT_IMAGE_MAX_H;
//   3. `thresholdAlpha`: alfa binario, come in un vero sprite pixel-art;
//   4. `dominantColor`: colore dominante, usato per accenti e fallback.

/** Dimensioni massime dell'immagine in pixel (= 2x la zona busto+testa dello sprite, 12x17). */
export const BOT_IMAGE_MAX_W = 24;
export const BOT_IMAGE_MAX_H = 34;
/** Peso massimo del file sorgente. */
export const BOT_IMAGE_SOURCE_MAX_BYTES = 10 * 1024 * 1024;
/** Tipi MIME accettati come sorgente. */
export const BOT_IMAGE_TYPES: readonly string[] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
/** Soglia dell'alfa binario (50%). */
export const ALPHA_THRESHOLD = 128;
/** Alfa minimo perche' un pixel conti nel ritaglio (ignora aloni sfumati). */
const CROP_ALPHA_MIN = 16;
/** Bit tenuti per canale nel colore dominante (i 4 piu' alti). */
const COLOR_BITS = 4;
const COLOR_SHIFT = 8 - COLOR_BITS;

/** Canali per pixel (RGBA) e loro posizioni. */
const STRIDE = 4;
const R = 0;
const G = 1;
const B = 2;
const A = 3;
const ALPHA_OPAQUE = 255;

/** Firma PNG, chunk IHDR e offset di larghezza/altezza (big-endian u32). */
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const PNG_IHDR = [0x49, 0x48, 0x44, 0x52];
const PNG_IHDR_OFFSET = 12;
const PNG_WIDTH_OFFSET = 16;
const PNG_HEIGHT_OFFSET = 20;
const PNG_HEADER_BYTES = 24;
/** 24 byte = 32 caratteri base64. */
const PNG_HEADER_CHARS = 32;

export interface Size {
  w: number;
  h: number;
}
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
/** Pixel RGBA non premoltiplicati, riga per riga (come ImageData). */
export interface PixelImage {
  data: Uint8ClampedArray;
  width: number;
  height: number;
}

/** Rettangolo minimo dei pixel con alfa >= `CROP_ALPHA_MIN`; null se non ce ne sono. */
export function opaqueBounds(img: PixelImage): Rect | null {
  const { data, width, height } = img;
  let minX = width;
  let minY = height;
  let maxX = -1;
  let maxY = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (data[(y * width + x) * STRIDE + A] < CROP_ALPHA_MIN) continue;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      maxY = y;
    }
  }
  if (maxX < 0) return null;
  return { x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1 };
}

/** Copia il rettangolo (limitato ai bordi dell'immagine). */
export function cropImage(img: PixelImage, rect: Rect): PixelImage {
  const x0 = Math.max(0, Math.min(img.width, rect.x));
  const y0 = Math.max(0, Math.min(img.height, rect.y));
  const w = Math.max(0, Math.min(img.width - x0, rect.w));
  const h = Math.max(0, Math.min(img.height - y0, rect.h));
  const data = new Uint8ClampedArray(w * h * STRIDE);
  for (let y = 0; y < h; y++) {
    const from = ((y0 + y) * img.width + x0) * STRIDE;
    data.set(img.data.subarray(from, from + w * STRIDE), y * w * STRIDE);
  }
  return { data, width: w, height: h };
}

/**
 * Dimensione massima con lo stesso rapporto d'aspetto dentro `maxW` x `maxH`.
 * Ingrandisce anche le immagini piccole (ogni corpo riempie la zona); arrotonda
 * con `Math.round` (8.5 -> 9) e ogni lato resta >= 1.
 */
export function fitSize(w: number, h: number, maxW: number, maxH: number): Size {
  const scale = Math.min(maxW / w, maxH / h);
  return {
    w: Math.max(1, Math.min(maxW, Math.round(w * scale))),
    h: Math.max(1, Math.min(maxH, Math.round(h * scale))),
  };
}

/** Sovrapposizione 1D tra [a0,a1) e [b0,b1). */
const overlap = (a0: number, a1: number, b0: number, b1: number): number =>
  Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));

/**
 * Ricampionamento a media d'area (filtro a scatola con copertura frazionaria),
 * valido in riduzione e in ingrandimento. I colori sono pesati per alfa x
 * copertura (niente bordi scuri dai pixel trasparenti); l'alfa e' la media
 * pesata per copertura. I pixel trasparenti in uscita hanno rgb 0.
 */
export function resample(img: PixelImage, size: Size): PixelImage {
  const { w, h } = size;
  const out = new Uint8ClampedArray(w * h * STRIDE);
  const sx = img.width / w;
  const sy = img.height / h;
  for (let y = 0; y < h; y++) {
    const y0 = y * sy;
    const y1 = y0 + sy;
    for (let x = 0; x < w; x++) {
      const x0 = x * sx;
      const x1 = x0 + sx;
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      let area = 0;
      for (let py = Math.floor(y0); py < Math.min(img.height, Math.ceil(y1)); py++) {
        const cy = overlap(y0, y1, py, py + 1);
        for (let px = Math.floor(x0); px < Math.min(img.width, Math.ceil(x1)); px++) {
          const cov = cy * overlap(x0, x1, px, px + 1);
          const i = (py * img.width + px) * STRIDE;
          const wa = cov * img.data[i + A];
          r += wa * img.data[i + R];
          g += wa * img.data[i + G];
          b += wa * img.data[i + B];
          a += wa;
          area += cov;
        }
      }
      const o = (y * w + x) * STRIDE;
      if (a > 0 && area > 0) {
        out[o + R] = Math.round(r / a);
        out[o + G] = Math.round(g / a);
        out[o + B] = Math.round(b / a);
        out[o + A] = Math.round(a / area);
      }
    }
  }
  return { data: out, width: w, height: h };
}

/** Alfa binario: sotto soglia (0,0,0,0), altrimenti alfa 255 e rgb invariato. Non muta l'input. */
export function thresholdAlpha(img: PixelImage, threshold: number = ALPHA_THRESHOLD): PixelImage {
  const data = new Uint8ClampedArray(img.data.length);
  for (let i = 0; i < data.length; i += STRIDE) {
    if (img.data[i + A] < threshold) continue;
    data[i + R] = img.data[i + R];
    data[i + G] = img.data[i + G];
    data[i + B] = img.data[i + B];
    data[i + A] = ALPHA_OPAQUE;
  }
  return { data, width: img.width, height: img.height };
}

const hex2 = (n: number): string => n.toString(16).padStart(2, "0");

/**
 * Colore dominante dei pixel con alfa >= `ALPHA_THRESHOLD`: canali quantizzati
 * ai 4 bit alti, vince il bucket piu' frequente (a pari merito il primo visto);
 * risultato = media dei colori reali del bucket, `#rrggbb` minuscolo.
 */
export function dominantColor(img: PixelImage): string | null {
  const buckets = new Map<number, { n: number; r: number; g: number; b: number }>();
  for (let i = 0; i < img.data.length; i += STRIDE) {
    if (img.data[i + A] < ALPHA_THRESHOLD) continue;
    const r = img.data[i + R];
    const g = img.data[i + G];
    const b = img.data[i + B];
    const key =
      (((r >> COLOR_SHIFT) << COLOR_BITS) | (g >> COLOR_SHIFT)) * (1 << COLOR_BITS) + (b >> COLOR_SHIFT);
    const bucket = buckets.get(key);
    if (bucket) {
      bucket.n++;
      bucket.r += r;
      bucket.g += g;
      bucket.b += b;
    } else {
      buckets.set(key, { n: 1, r, g, b });
    }
  }
  let best: { n: number; r: number; g: number; b: number } | null = null;
  for (const bucket of buckets.values()) if (!best || bucket.n > best.n) best = bucket;
  if (!best) return null;
  const mean = (sum: number): string => hex2(Math.round(sum / best!.n));
  return `#${mean(best.r)}${mean(best.g)}${mean(best.b)}`;
}

export interface Pixelated {
  image: PixelImage;
  color: string;
}

/** Pipeline completa; null se non resta nessun pixel opaco. */
export function pixelate(img: PixelImage): Pixelated | null {
  const bounds = opaqueBounds(img);
  if (!bounds) return null;
  const cropped = cropImage(img, bounds);
  const size = fitSize(cropped.width, cropped.height, BOT_IMAGE_MAX_W, BOT_IMAGE_MAX_H);
  const image = thresholdAlpha(resample(cropped, size));
  const color = dominantColor(image);
  return color === null ? null : { image, color };
}

/** Dimensioni lette dall'intestazione IHDR di un PNG in base64; null se non e' un PNG valido. */
export function pngSize(base64: string): Size | null {
  let raw: string;
  try {
    raw = atob(base64.slice(0, PNG_HEADER_CHARS));
  } catch {
    return null;
  }
  if (raw.length < PNG_HEADER_BYTES) return null;
  const bytes = Array.from(raw.slice(0, PNG_HEADER_BYTES), (c) => c.charCodeAt(0));
  const matches = (expected: number[], offset: number) => expected.every((v, i) => bytes[offset + i] === v);
  if (!matches(PNG_SIGNATURE, 0) || !matches(PNG_IHDR, PNG_IHDR_OFFSET)) return null;
  const u32 = (o: number) =>
    ((bytes[o] << 24) | (bytes[o + 1] << 16) | (bytes[o + 2] << 8) | bytes[o + 3]) >>> 0;
  return { w: u32(PNG_WIDTH_OFFSET), h: u32(PNG_HEIGHT_OFFSET) };
}
