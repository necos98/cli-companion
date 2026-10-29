import type { CSSProperties } from "react";
import { ROOM_WIDTH } from "../../lib/office";

/**
 * Una "unita' stanza" in CSS. La stanza e' un container (`container-type:
 * inline-size`): `cqw` e' relativo alla sua larghezza, quindi tutto cio' che e'
 * misurato in unita' scala con la stanza. La variabile si dichiara su un
 * antenato della stanza stessa (il container non risolve le proprie `cqw`).
 */
export const UNIT_VAR = "--office-u";

/** Lunghezza di `n` unita' stanza. */
export const roomUnits = (n: number): string => `calc(var(${UNIT_VAR}) * ${n})`;

/** Stile da mettere sugli elementi dentro il container della stanza. */
export const ROOM_UNIT_STYLE = {
  [UNIT_VAR]: `calc(100cqw / ${ROOM_WIDTH})`,
} as CSSProperties;

/** Posizione in percentuale di un valore su un totale. */
export const percent = (value: number, total: number): string => `${(value / total) * 100}%`;
