import type { CSSProperties } from "react";

interface Props {
  x: number;
  y: number;
  w: number;
  h: number;
  /** Colore di riempimento. */
  c: string;
  className?: string;
  style?: CSSProperties;
}

/** Un rettangolo della pixel-art (le scene SVG sono fatte solo di questi). */
export function Px({ x, y, w, h, c, className, style }: Props) {
  return <rect x={x} y={y} width={w} height={h} fill={c} className={className} style={style} />;
}
