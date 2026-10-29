import { useCallback, useRef, useState } from "react";
import type { ChangeEvent } from "react";
import { BOT_IMAGE_TYPES } from "../../lib/botImage";
import type { BotImage } from "../../lib/bots";
import { pixelateFile } from "./pixelateFile";

/** Valore dell'attributo `accept` del selettore di file. */
export const BOT_IMAGE_ACCEPT = BOT_IMAGE_TYPES.join(",");

/** Immagine pixelata in attesa di conferma, per un nome della pool. */
export interface BotImageCandidate {
  name: string;
  image: BotImage;
}

/**
 * Scelta di un'immagine per un nome: apre il selettore di file, pixelizza e
 * tiene il risultato come candidato finche' l'utente non conferma o annulla.
 * Gli errori vanno a `onError` (la riga d'errore della pagina).
 */
export function useBotImagePicker(onError: (message: string | null) => void) {
  const inputRef = useRef<HTMLInputElement>(null);
  const targetRef = useRef<string | null>(null);
  /** Numero dell'ultima scelta: scarta i risultati di scelte superate o annullate. */
  const attemptRef = useRef(0);
  const [candidate, setCandidate] = useState<BotImageCandidate | null>(null);
  const [pixelating, setPixelating] = useState(false);

  const pick = useCallback((name: string) => {
    targetRef.current = name;
    inputRef.current?.click();
  }, []);

  const cancel = useCallback(() => {
    attemptRef.current++;
    setPixelating(false);
    setCandidate(null);
  }, []);

  const onFileChange = useCallback(
    async (e: ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      // Azzera il campo: lo stesso file si puo' scegliere di nuovo.
      e.target.value = "";
      const name = targetRef.current;
      if (!file || name === null) return;
      const attempt = ++attemptRef.current;
      onError(null);
      setPixelating(true);
      try {
        const image = await pixelateFile(file);
        if (attempt === attemptRef.current) setCandidate({ name, image });
      } catch (err) {
        if (attempt !== attemptRef.current) return;
        setCandidate(null);
        onError(err instanceof Error ? err.message : String(err));
      } finally {
        if (attempt === attemptRef.current) setPixelating(false);
      }
    },
    [onError],
  );

  return { inputRef, candidate, pixelating, pick, cancel, onFileChange };
}
