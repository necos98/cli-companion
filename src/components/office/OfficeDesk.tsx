import { memo } from "react";
import type { Desk } from "../../lib/office";
import { OFFICE_COLORS as C } from "./palette";
import { Px } from "./Px";

/** Cosa mostra il monitor: spento, acceso (bot al lavoro) o in errore (fallito). */
export type ScreenState = "off" | "on" | "error";

/** Righe di "codice" sullo schermo: [x, larghezza]. */
const CODE_LINES: ReadonlyArray<readonly [number, number]> = [
  [0.15, 0.7],
  [0.3, 0.9],
  [0.3, 0.5],
  [0.15, 0.8],
  [0.15, 0.4],
];
const LINE_STEP = 0.17;
/** Ritardo tra una riga di codice e la successiva (s). */
const LINE_STAGGER = 0.21;

const SCREEN = {
  off: { glass: C.screenOff, text: null },
  on: { glass: C.screenOn, text: C.screenText },
  error: { glass: C.screenError, text: C.screenErrorText },
} as const;

/** Mezza larghezza della sedia (u). */
const CHAIR_HALF = 0.55;
const CHAIR_BACK_HEIGHT = 1.4;

interface Props {
  desk: Desk;
  screen: ScreenState;
}

/** Scrivania col monitor sulla meta' sinistra, la tastiera a destra e la sedia dove siede il bot. */
function OfficeDesk({ desk, screen }: Props) {
  const { glass, text } = SCREEN[screen];
  const { seat } = desk;
  // Lo schienale sta dalla parte opposta a dove guarda il bot.
  const backX = desk.facing === -1 ? seat.x + CHAIR_HALF - 0.2 : seat.x - CHAIR_HALF;
  return (
    <>
      <Px x={seat.x - CHAIR_HALF} y={seat.y - 0.6} w={2 * CHAIR_HALF} h={0.25} c={C.steelLight} />
      <Px x={backX} y={seat.y - 0.6 - CHAIR_BACK_HEIGHT} w={0.2} h={CHAIR_BACK_HEIGHT} c={C.steel} />
      <Px x={seat.x - 0.1} y={seat.y - 0.35} w={0.2} h={0.25} c={C.steelDark} />
      <Px x={seat.x - CHAIR_HALF} y={seat.y - 0.1} w={2 * CHAIR_HALF} h={0.1} c={C.ink} />
      <g transform={`translate(${desk.x} ${desk.y})`}>
        {screen === "on" && <Px x={0} y={0.05} w={1.9} h={1.5} c={C.screenText} style={{ opacity: 0.12 }} />}
        <Px x={0.2} y={0.15} w={1.5} h={1.2} c={C.steelDark} />
        <Px x={0.3} y={0.25} w={1.3} h={0.95} c={glass} />
        {text &&
          CODE_LINES.map(([x, w], i) => (
            <Px
              key={i}
              x={0.3 + x}
              y={0.35 + i * LINE_STEP}
              w={w}
              h={0.08}
              c={text}
              className={screen === "on" ? "office-code" : undefined}
              style={{ animationDelay: `${i * LINE_STAGGER}s` }}
            />
          ))}
        <Px x={0.8} y={1.35} w={0.3} h={0.25} c={C.steelLight} />
        <Px x={0.55} y={1.55} w={0.8} h={0.1} c={C.steelLight} />
        <Px x={0} y={1.65} w={desk.w} h={0.3} c={C.wood} />
        <Px x={0} y={1.65} w={desk.w} h={0.08} c={C.woodLight} />
        <Px x={0.1} y={1.95} w={0.25} h={desk.h - 1.95} c={C.woodDark} />
        <Px x={desk.w - 0.35} y={1.95} w={0.25} h={desk.h - 1.95} c={C.woodDark} />
        <Px x={0.35} y={1.95} w={desk.w - 0.7} h={0.12} c={C.woodDark} />
        <Px x={2.1} y={1.5} w={1.0} h={0.15} c={C.ink} />
        <Px x={2.15} y={1.53} w={0.9} h={0.04} c={C.steelLight} />
        <Px x={1.7} y={1.35} w={0.25} h={0.3} c={C.mug} />
      </g>
    </>
  );
}

export default memo(OfficeDesk);
