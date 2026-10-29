import { memo } from "react";
import type { ReactElement } from "react";
import type { Furniture, FurnitureKind } from "../../lib/office";
import { OFFICE_COLORS as C } from "./palette";
import { Px } from "./Px";

interface Size {
  w: number;
  h: number;
}

/** Spessore delle cornici (u). */
const FRAME = 0.15;
/** Colori dei LED dei server, a rotazione. */
const LEDS = [C.ledGreen, C.ledAmber, C.ledBlue] as const;
const SERVER_UNITS = 5;
const SERVER_UNIT_STEP = 0.8;
/** Ritardo tra un LED e il successivo (s): lampeggi sfasati. */
const LED_STAGGER = 0.23;
const LED_PHASES = 7;

function Board({ w, h }: Size) {
  return (
    <>
      <Px x={0} y={0} w={w} h={h} c={C.steelLight} />
      <Px x={FRAME} y={FRAME} w={w - 2 * FRAME} h={h - 2 * FRAME} c={C.board} />
      <Px x={FRAME} y={h - 0.45} w={w - 2 * FRAME} h={0.3} c={C.steel} />
      {/* Appunti e post-it (il nome del workspace e' testo HTML sopra la lavagna) */}
      <Px x={0.5} y={1.7} w={3.2} h={0.12} c={C.boardInk} />
      <Px x={0.5} y={2.0} w={4.2} h={0.12} c={C.boardInk} />
      <Px x={0.5} y={2.3} w={2.4} h={0.12} c={C.boardInk} />
      <Px x={w - 1.9} y={1.5} w={0.6} h={0.6} c={C.noteYellow} />
      <Px x={w - 1.15} y={1.7} w={0.6} h={0.6} c={C.noteGreen} />
      <Px x={w - 1.9} y={2.25} w={0.45} h={0.4} c={C.noteBlue} />
    </>
  );
}

const SKYLINE: ReadonlyArray<readonly [x: number, w: number, h: number]> = [
  [0.2, 0.9, 0.9],
  [1.2, 0.7, 1.4],
  [2.0, 1.0, 0.7],
  [3.1, 0.8, 1.2],
  [4.0, 0.9, 0.8],
  [4.9, 0.7, 1.3],
];
/** Finestrelle accese dello skyline: [indice palazzo, dx, dy dal tetto]. */
const SKYLINE_LIGHTS: ReadonlyArray<readonly [number, number, number]> = [
  [1, 0.2, 0.3],
  [1, 0.4, 0.7],
  [3, 0.3, 0.3],
  [5, 0.2, 0.4],
];

function Window({ w, h }: Size) {
  const inner = { x: FRAME + 0.05, y: FRAME + 0.05, w: w - 0.4, h: h - 0.65 };
  const band = inner.h / 3;
  const ground = inner.y + inner.h;
  return (
    <>
      <Px x={0} y={0} w={w} h={h - 0.2} c={C.steelLight} />
      <Px x={inner.x} y={inner.y} w={inner.w} h={band} c={C.skyTop} />
      <Px x={inner.x} y={inner.y + band} w={inner.w} h={band} c={C.skyMid} />
      <Px x={inner.x} y={inner.y + 2 * band} w={inner.w} h={band} c={C.skyLow} />
      <Px x={w - 1.4} y={0.5} w={0.45} h={0.45} c={C.moon} />
      {SKYLINE.map(([x, bw, bh], i) => (
        <Px key={i} x={inner.x + x} y={ground - bh} w={bw} h={bh} c={C.skyline} />
      ))}
      {SKYLINE_LIGHTS.map(([building, dx, dy], i) => {
        const [x, , bh] = SKYLINE[building];
        return <Px key={i} x={inner.x + x + dx} y={ground - bh + dy} w={0.15} h={0.15} c={C.skylineLit} />;
      })}
      {/* Montanti e davanzale */}
      <Px x={w / 2 - 0.1} y={inner.y} w={0.2} h={inner.h} c={C.steelLight} />
      <Px x={inner.x} y={inner.y + inner.h / 2 - 0.1} w={inner.w} h={0.2} c={C.steelLight} />
      <Px x={0} y={h - 0.2} w={w} h={0.2} c={C.wood} />
    </>
  );
}

function ServerRack({ w, h }: Size) {
  return (
    <>
      <Px x={0} y={0} w={w} h={h} c={C.steelDark} />
      <Px x={0} y={h - 0.3} w={w} h={0.3} c={C.ink} />
      {Array.from({ length: SERVER_UNITS }, (_, i) => {
        const y = 0.2 + i * SERVER_UNIT_STEP;
        return (
          <g key={i}>
            <Px x={0.2} y={y} w={w - 0.4} h={0.7} c={C.steel} />
            <Px x={0.3} y={y + 0.1} w={w - 0.6} h={0.5} c={C.ink} />
            {[0, 1].map((j) => (
              <Px
                key={j}
                x={0.45 + j * 0.3}
                y={y + 0.25}
                w={0.15}
                h={0.15}
                c={LEDS[(i + j) % LEDS.length]}
                className="office-led"
                style={{ animationDelay: `${((i * 3 + j * 5) % LED_PHASES) * LED_STAGGER}s` }}
              />
            ))}
            <Px x={w - 1.1} y={y + 0.25} w={0.6} h={0.12} c={C.steelLight} />
          </g>
        );
      })}
    </>
  );
}

function CoffeeCounter({ w, h }: Size) {
  const topY = h - 1.3;
  return (
    <>
      {/* Macchina */}
      <Px x={0.3} y={0.2} w={1.2} h={topY - 0.2} c={C.steel} />
      <Px x={0.3} y={0.2} w={1.2} h={0.3} c={C.steelDark} />
      <Px x={0.55} y={0.65} w={0.45} h={0.2} c={C.ledGreen} />
      <Px x={0.6} y={topY - 0.45} w={0.4} h={0.4} c={C.mug} />
      <Px x={0.7} y={topY - 0.35} w={0.2} h={0.1} c={C.coffee} />
      <Px x={0.7} y={0} w={0.12} h={0.12} c={C.mug} className="office-steam" />
      <Px x={0.95} y={0} w={0.12} h={0.12} c={C.mug} className="office-steam office-steam--late" />
      {/* Barattoli e tazze sul piano */}
      <Px x={1.8} y={topY - 0.6} w={0.5} h={0.6} c={C.woodDark} />
      <Px x={2.4} y={topY - 0.4} w={0.4} h={0.4} c={C.mug} />
      <Px x={2.55} y={topY - 0.55} w={0.4} h={0.15} c={C.mug} />
      {/* Mobile */}
      <Px x={0} y={topY} w={w} h={0.25} c={C.woodLight} />
      <Px x={0} y={topY + 0.25} w={w} h={h - topY - 0.25} c={C.woodDark} />
      <Px x={w / 2 - 0.05} y={topY + 0.25} w={0.1} h={h - topY - 0.25} c={C.wood} />
    </>
  );
}

function Plant({ w, h }: Size) {
  const potTop = h - 0.9;
  const mid = w / 2;
  return (
    <>
      <Px x={mid - 0.08} y={0.7} w={0.16} h={potTop - 0.7} c={C.leafDark} />
      <Px x={mid - 0.35} y={0} w={0.7} h={0.6} c={C.leaf} />
      <Px x={0} y={0.5} w={0.6} h={0.45} c={C.leaf} />
      <Px x={w - 0.6} y={0.35} w={0.6} h={0.45} c={C.leafDark} />
      <Px x={0.15} y={1.0} w={0.5} h={0.4} c={C.leafDark} />
      <Px x={w - 0.65} y={0.95} w={0.5} h={0.4} c={C.leaf} />
      <Px x={0.1} y={potTop} w={w - 0.2} h={0.25} c={C.pot} />
      <Px x={0.2} y={potTop + 0.25} w={w - 0.4} h={h - potTop - 0.25} c={C.potDark} />
    </>
  );
}

function Sofa({ w, h }: Size) {
  const arm = 0.5;
  const cushions = 3;
  const cushionW = (w - 2 * arm) / cushions;
  return (
    <>
      <Px x={0.2} y={0} w={w - 0.4} h={1.2} c={C.sofa} />
      <Px x={0.2} y={0} w={w - 0.4} h={0.2} c={C.sofaLight} />
      <Px x={0} y={0.5} w={arm} h={h - 0.7} c={C.sofaDark} />
      <Px x={w - arm} y={0.5} w={arm} h={h - 0.7} c={C.sofaDark} />
      {Array.from({ length: cushions }, (_, i) => (
        <g key={i}>
          <Px x={arm + i * cushionW + 0.03} y={1.0} w={cushionW - 0.06} h={0.7} c={C.sofaLight} />
          <Px x={arm + i * cushionW + 0.03} y={1.55} w={cushionW - 0.06} h={0.15} c={C.sofa} />
        </g>
      ))}
      <Px x={0.2} y={h - 0.2} w={0.2} h={0.2} c={C.ink} />
      <Px x={w - 0.4} y={h - 0.2} w={0.2} h={0.2} c={C.ink} />
    </>
  );
}

const PIECES: Record<FurnitureKind, (size: Size) => ReactElement> = {
  board: Board,
  window: Window,
  server: ServerRack,
  coffee: CoffeeCounter,
  plant: Plant,
  sofa: Sofa,
};

interface Props {
  item: Furniture;
}

/** Un mobile della stanza, disegnato esattamente nel suo rettangolo del layout. */
function OfficeFurniture({ item }: Props) {
  const Piece = PIECES[item.kind];
  return (
    <g transform={`translate(${item.x} ${item.y})`}>
      <Piece w={item.w} h={item.h} />
    </g>
  );
}

export default memo(OfficeFurniture);
