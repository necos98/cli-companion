// Strato canvas dell'import: decodifica il file, lo passa alla pipeline pura di
// `lib/botImage.ts` e riesporta il risultato come PNG base64. Gli errori sono
// messaggi italiani, mostrati cosi' come sono nella pagina Bot.

import { BOT_IMAGE_SOURCE_MAX_BYTES, BOT_IMAGE_TYPES, pixelate } from "../../lib/botImage";
import type { PixelImage } from "../../lib/botImage";
import type { BotImage } from "../../lib/bots";

/** Lato lungo massimo con cui si legge il sorgente (limita la memoria con immagini enormi). */
const PRESCALE_MAX = 512;
const PNG_DATA_PREFIX = "data:image/png;base64,";
const MB = 1024 * 1024;

const ERR_TOO_BIG = `L’immagine supera i ${BOT_IMAGE_SOURCE_MAX_BYTES / MB} MB`;
const ERR_TYPE = "Formato non supportato: usa PNG, JPEG, WebP o GIF";
const ERR_UNREADABLE = "Impossibile leggere l’immagine";
const ERR_EMPTY = "L’immagine è completamente trasparente";

function contextOf(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error(ERR_UNREADABLE);
  return ctx;
}

/** Pixel del file, ridotti (mantenendo le proporzioni) a `PRESCALE_MAX` sul lato lungo. */
async function readPixels(file: File): Promise<PixelImage> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    throw new Error(ERR_UNREADABLE);
  }
  try {
    const scale = Math.min(1, PRESCALE_MAX / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const ctx = contextOf(canvas);
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    return ctx.getImageData(0, 0, canvas.width, canvas.height);
  } finally {
    bitmap.close();
  }
}

/** PNG (base64, senza prefisso `data:`) dei pixel. */
function toPngBase64(image: PixelImage): string {
  const canvas = document.createElement("canvas");
  canvas.width = image.width;
  canvas.height = image.height;
  contextOf(canvas).putImageData(new ImageData(new Uint8ClampedArray(image.data), image.width, image.height), 0, 0);
  return canvas.toDataURL("image/png").slice(PNG_DATA_PREFIX.length);
}

/** Trasforma un file immagine nel corpo pixelato di un bot. */
export async function pixelateFile(file: File): Promise<BotImage> {
  if (file.size > BOT_IMAGE_SOURCE_MAX_BYTES) throw new Error(ERR_TOO_BIG);
  if (!BOT_IMAGE_TYPES.includes(file.type)) throw new Error(ERR_TYPE);
  const result = pixelate(await readPixels(file));
  if (!result) throw new Error(ERR_EMPTY);
  return { png: toPngBase64(result.image), color: result.color };
}
