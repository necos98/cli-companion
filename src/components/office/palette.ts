// Colori dell'ufficio: una palette scura e fredda, in tinta con i token del
// tema (`--surface`, `--pane-area`, `--border-strong`), con pochi accenti caldi
// (legno, caffe', piante) per dare vita alla stanza.

export const OFFICE_COLORS = {
  // Pareti e pavimento
  wall: "#172433",
  wallTrim: "#22364b",
  wallPanel: "#111d2a",
  skirting: "#0a121b",
  floorLight: "#121d29",
  floorDark: "#0f1822",
  carpet: "#0c141c",
  carpetEdge: "#1a2a3b",
  rug: "#1b2c42",
  rugEdge: "#2a4263",
  shadow: "#04080d",
  // Legno
  wood: "#a07850",
  woodLight: "#b8906a",
  woodDark: "#6b4a2f",
  // Metallo e plastica
  steel: "#334155",
  steelLight: "#475569",
  steelDark: "#1e293b",
  ink: "#0f172a",
  // Lavagna e appunti
  board: "#b9c5d1",
  boardInk: "#64748b",
  noteYellow: "#fbbf24",
  noteGreen: "#34d399",
  noteBlue: "#60a5fa",
  // Finestra
  skyTop: "#0f2a47",
  skyMid: "#15395f",
  skyLow: "#1f5183",
  skyline: "#0b1c30",
  skylineLit: "#fcd34d",
  moon: "#e2e8f0",
  // Schermi
  screenOff: "#0b1220",
  screenOn: "#0e2a40",
  screenText: "#7dd3fc",
  screenError: "#3b0f14",
  screenErrorText: "#fca5a5",
  // Server
  ledGreen: "#4ade80",
  ledAmber: "#fbbf24",
  ledBlue: "#38bdf8",
  // Caffe'
  mug: "#e2e8f0",
  coffee: "#78350f",
  // Piante
  leaf: "#22c55e",
  leafDark: "#15803d",
  pot: "#c2410c",
  potDark: "#9a3412",
  // Divano
  sofa: "#3b4a7a",
  sofaLight: "#4d609c",
  sofaDark: "#2a3559",
  // Orologio
  clockFace: "#dbe4ee",
} as const;

/** Colori di ciò che il bot porta con sé (tazza, vapore, cartellina). */
export const SPRITE_PROPS = {
  mug: "#e2e8f0",
  steam: "#cbd5e1",
  clipboard: "#a16207",
  paper: "#e2e8f0",
} as const;
