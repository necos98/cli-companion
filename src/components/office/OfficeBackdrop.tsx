import { memo, useId } from "react";
import { DESK_AREA_Y } from "../../lib/office";
import { OFFICE_COLORS as C } from "./palette";
import { Px } from "./Px";

/** Altezza della parete di fondo (u). */
const WALL_HEIGHT = 5;
/** La moquette delle scrivanie inizia poco sopra i monitor. */
const CARPET_MARGIN = 0.8;
/** Lato di una piastrella del pavimento (u). */
const TILE = 1;

interface Props {
  width: number;
  height: number;
}

/** Parete, pavimento a piastrelle, moquette delle scrivanie, tappeti e orologio. */
function OfficeBackdrop({ width, height }: Props) {
  // Il pattern ha un id proprio: piu' stanze nella stessa pagina non si pestano.
  const floorId = `office-floor-${useId().replace(/:/g, "")}`;
  const carpetY = DESK_AREA_Y - CARPET_MARGIN;
  return (
    <>
      <defs>
        <pattern id={floorId} width={TILE * 2} height={TILE * 2} patternUnits="userSpaceOnUse">
          <Px x={0} y={0} w={TILE} h={TILE} c={C.floorDark} />
          <Px x={TILE} y={0} w={TILE} h={TILE} c={C.floorLight} />
          <Px x={0} y={TILE} w={TILE} h={TILE} c={C.floorLight} />
          <Px x={TILE} y={TILE} w={TILE} h={TILE} c={C.floorDark} />
        </pattern>
      </defs>
      <rect x={0} y={WALL_HEIGHT} width={width} height={height - WALL_HEIGHT} fill={`url(#${floorId})`} />
      {/* Parete: cimasa, pannello basso, battiscopa e ombra sul pavimento */}
      <Px x={0} y={0} w={width} h={WALL_HEIGHT} c={C.wall} />
      <Px x={0} y={0} w={width} h={0.3} c={C.wallTrim} />
      <Px x={0} y={3.7} w={width} h={1.1} c={C.wallPanel} />
      <Px x={0} y={3.7} w={width} h={0.12} c={C.wallTrim} />
      <Px x={0} y={4.8} w={width} h={0.2} c={C.skirting} />
      <Px x={0} y={WALL_HEIGHT} w={width} h={0.25} c={C.shadow} style={{ opacity: 0.35 }} />
      {/* Moquette della zona scrivanie */}
      <Px x={0} y={carpetY} w={width} h={height - carpetY} c={C.carpet} />
      <Px x={0} y={carpetY} w={width} h={0.12} c={C.carpetEdge} />
      {/* Tappeto sotto il divano e zona server */}
      <Px x={10.4} y={7.3} w={6.2} h={1.7} c={C.rugEdge} />
      <Px x={10.6} y={7.5} w={5.8} h={1.3} c={C.rug} />
      <Px x={20.3} y={7} w={3.4} h={2.2} c={C.rugEdge} />
      <Px x={20.5} y={7.2} w={3} h={1.8} c={C.rug} />
      {/* Orologio */}
      <Px x={17.7} y={1.2} w={1.5} h={1.5} c={C.steelLight} />
      <Px x={17.85} y={1.35} w={1.2} h={1.2} c={C.clockFace} />
      <Px x={18.4} y={1.5} w={0.1} h={0.6} c={C.ink} />
      <Px x={18.4} y={2} w={0.4} h={0.1} c={C.ink} />
    </>
  );
}

export default memo(OfficeBackdrop);
