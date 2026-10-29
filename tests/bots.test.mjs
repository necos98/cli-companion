import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  ACCESSORIES,
  ANTENNAS,
  BOT_GRID,
  BOT_NAME_MAX,
  BOT_PALETTE,
  EYE_STYLES,
  HEAD_SHAPES,
  assignRestoredNames,
  botLook,
  botPixels,
  pickBotName,
  sanitizeBotName,
} from "../src/lib/bots.ts";

const POOL = ["Bolt", "Pixel", "Gizmo"];

describe("pickBotName", () => {
  it("sceglie dalla pool in base al generatore casuale", () => {
    assert.equal(pickBotName(POOL, [], () => 0), "Bolt");
    assert.equal(pickBotName(POOL, [], () => 0.5), "Pixel");
    assert.equal(pickBotName(POOL, [], () => 0.99), "Gizmo");
    assert.equal(pickBotName(POOL, [], () => 1), "Gizmo");
  });

  it("salta i nomi usati dai tab aperti, senza badare alle maiuscole", () => {
    assert.equal(pickBotName(POOL, ["Bolt", "PIXEL"], () => 0), "Gizmo");
    assert.equal(pickBotName(POOL, new Set(["bolt"]), () => 0), "Pixel");
  });

  it("non ripete mai un nome finche la pool ne ha di liberi", () => {
    const taken = [];
    for (let i = 0; i < POOL.length; i++) taken.push(pickBotName(POOL, taken));
    assert.equal(new Set(taken).size, POOL.length);
  });

  it("a pool esaurita aggiunge un suffisso numerico libero", () => {
    assert.equal(pickBotName(POOL, POOL, () => 0), "Bolt 2");
    assert.equal(pickBotName(POOL, [...POOL, "Bolt 2"], () => 0), "Bolt 3");
    assert.equal(pickBotName(["Bolt"], ["bolt", "BOLT 2"], () => 0), "Bolt 3");
  });

  it("a pool vuota usa Bot 1, Bot 2...", () => {
    assert.equal(pickBotName([], []), "Bot 1");
    assert.equal(pickBotName([], ["Bot 1"]), "Bot 2");
    assert.equal(pickBotName([], ["Bot 1", "Bot 3"]), "Bot 2");
  });

  it("un nome con suffisso non occupa il nome semplice", () => {
    assert.equal(pickBotName(["Bolt"], ["Bolt 2"], () => 0), "Bolt");
  });
});

describe("sanitizeBotName", () => {
  it("accetta un nome normale e lo trimma", () => {
    assert.equal(sanitizeBotName("  Bolt "), "Bolt");
    assert.equal(sanitizeBotName("Bolt 2"), "Bolt 2");
  });

  it("scarta valori non stringa o vuoti", () => {
    for (const v of [undefined, null, 5, {}, "", "   ", "\n\t"]) {
      assert.equal(sanitizeBotName(v), undefined);
    }
  });

  it("toglie i caratteri di controllo", () => {
    assert.equal(sanitizeBotName("Bo\u0000l\nt\u007f"), "Bolt");
  });

  it("limita la lunghezza in caratteri, non in unita UTF-16", () => {
    assert.equal(sanitizeBotName("a".repeat(100))?.length, BOT_NAME_MAX + 8);
    assert.equal(Array.from(sanitizeBotName("🤖".repeat(100), 5)).length, 5);
  });

  it("non lascia spazi finali dopo il taglio", () => {
    assert.equal(sanitizeBotName("abc def", 4), "abc");
  });
});

describe("assignRestoredNames", () => {
  it("i tab con nome valido lo tengono", () => {
    const names = assignRestoredNames(
      [
        { tabId: "a", botName: "Bolt" },
        { tabId: "b", botName: "Pixel" },
      ],
      POOL,
    );
    assert.equal(names.get("a"), "Bolt");
    assert.equal(names.get("b"), "Pixel");
  });

  it("il secondo tab con lo stesso nome ne riceve uno nuovo", () => {
    const names = assignRestoredNames(
      [
        { tabId: "a", botName: "Bolt" },
        { tabId: "b", botName: "bolt" },
      ],
      POOL,
      () => 0,
    );
    assert.equal(names.get("a"), "Bolt");
    assert.equal(names.get("b"), "Pixel");
  });

  it("i tab senza nome (sessioni vecchie) ne ricevono uno, tutti distinti", () => {
    const names = assignRestoredNames(
      [{ tabId: "a" }, { tabId: "b", botName: "Bolt" }, { tabId: "c" }],
      POOL,
      () => 0,
    );
    assert.equal(names.get("b"), "Bolt");
    assert.equal(new Set(names.values()).size, 3);
  });

  it("un nome salvato fuori pool e mantenuto", () => {
    const names = assignRestoredNames([{ tabId: "a", botName: "Custom" }], POOL);
    assert.equal(names.get("a"), "Custom");
  });

  it("nessun tab, nessun nome", () => {
    assert.equal(assignRestoredNames([], POOL).size, 0);
  });
});

describe("botLook", () => {
  it("e deterministico: stesso nome, stesso aspetto", () => {
    assert.deepEqual(botLook("Bolt"), botLook("Bolt"));
    assert.deepEqual(botLook("Bolt"), botLook("  bolt "));
  });

  it("i valori restano negli intervalli previsti", () => {
    for (const name of ["Bolt", "Pixel", "Zed", "x", "Bot 12", "🤖", "é".repeat(24)]) {
      const look = botLook(name);
      assert.ok(BOT_PALETTE.includes(look.color), name);
      assert.ok(Number.isInteger(look.head) && look.head >= 0 && look.head < HEAD_SHAPES, name);
      assert.ok(Number.isInteger(look.antenna) && look.antenna >= 0 && look.antenna < ANTENNAS, name);
      assert.ok(Number.isInteger(look.eyes) && look.eyes >= 0 && look.eyes < EYE_STYLES, name);
      assert.ok(
        Number.isInteger(look.accessory) && look.accessory >= 0 && look.accessory < ACCESSORIES,
        name,
      );
    }
  });

  it("nomi diversi producono aspetti vari", () => {
    const names = ["Bolt", "Pixel", "Gizmo", "Sprocket", "Widget", "Byte", "Nova", "Cog", "Rusty", "Chip"];
    const looks = new Set(names.map((n) => JSON.stringify(botLook(n))));
    assert.ok(looks.size >= 8);
    const colors = new Set(names.map((n) => botLook(n).color));
    assert.ok(colors.size >= 4);
  });
});

describe("botPixels", () => {
  it("ogni combinazione resta nella griglia e ha corpo e occhi", () => {
    for (let head = 0; head < HEAD_SHAPES; head++)
      for (let antenna = 0; antenna < ANTENNAS; antenna++)
        for (let eyes = 0; eyes < EYE_STYLES; eyes++)
          for (let accessory = 0; accessory < ACCESSORIES; accessory++) {
            const rects = botPixels({ color: BOT_PALETTE[0], head, antenna, eyes, accessory });
            for (const r of rects) {
              assert.ok(r.x >= 0 && r.y >= 0 && r.w > 0 && r.h > 0);
              assert.ok(r.x + r.w <= BOT_GRID && r.y + r.h <= BOT_GRID);
            }
            assert.ok(rects.some((r) => r.tone === "body"));
            assert.ok(rects.some((r) => r.tone === "dark"));
          }
  });

  it("lo stesso aspetto produce lo stesso disegno", () => {
    assert.deepEqual(botPixels(botLook("Bolt")), botPixels(botLook("Bolt")));
  });
});
