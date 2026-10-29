import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  ALPHA_THRESHOLD,
  BOT_IMAGE_MAX_H,
  BOT_IMAGE_MAX_W,
  cropImage,
  dominantColor,
  fitSize,
  opaqueBounds,
  pixelate,
  pngSize,
  resample,
  thresholdAlpha,
} from "../src/lib/botImage.ts";

/** Immagine da una funzione (x, y) -> [r, g, b, a]. */
function make(width, height, fn) {
  const data = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++) data.set(fn(x, y), (y * width + x) * 4);
  return { data, width, height };
}
const solid = (w, h, p) => make(w, h, () => p);
const px = (img, x, y) => Array.from(img.data.slice((y * img.width + x) * 4, (y * img.width + x) * 4 + 4));
const CLEAR = [0, 0, 0, 0];
const RED = [255, 0, 0, 255];

describe("opaqueBounds", () => {
  it("ignora i margini trasparenti", () => {
    const img = make(6, 5, (x, y) => (x >= 2 && x <= 3 && y >= 1 && y <= 3 ? RED : CLEAR));
    assert.deepEqual(opaqueBounds(img), { x: 2, y: 1, w: 2, h: 3 });
  });
  it("tutto trasparente o vuota: null", () => {
    assert.equal(opaqueBounds(solid(3, 3, CLEAR)), null);
    assert.equal(opaqueBounds({ data: new Uint8ClampedArray(0), width: 0, height: 0 }), null);
  });
  it("ignora l'alone sotto la soglia di ritaglio", () => {
    const img = make(5, 5, (x, y) => (x === 2 && y === 2 ? RED : x === 0 ? [255, 0, 0, 15] : CLEAR));
    assert.deepEqual(opaqueBounds(img), { x: 2, y: 2, w: 1, h: 1 });
    assert.deepEqual(opaqueBounds(solid(2, 2, [0, 0, 0, 16])), { x: 0, y: 0, w: 2, h: 2 });
  });
  it("pixel singolo e immagine piena", () => {
    assert.deepEqual(opaqueBounds(make(4, 4, (x, y) => (x === 3 && y === 0 ? RED : CLEAR))), {
      x: 3,
      y: 0,
      w: 1,
      h: 1,
    });
    assert.deepEqual(opaqueBounds(solid(3, 2, RED)), { x: 0, y: 0, w: 3, h: 2 });
  });
});

describe("cropImage", () => {
  it("copia il rettangolo", () => {
    const img = make(4, 4, (x, y) => [x, y, 0, 255]);
    const c = cropImage(img, { x: 1, y: 2, w: 2, h: 2 });
    assert.equal(c.width, 2);
    assert.equal(c.height, 2);
    assert.deepEqual(px(c, 0, 0), [1, 2, 0, 255]);
    assert.deepEqual(px(c, 1, 1), [2, 3, 0, 255]);
  });
  it("limita il rettangolo ai bordi", () => {
    const c = cropImage(solid(3, 3, RED), { x: 2, y: 2, w: 5, h: 5 });
    assert.equal(c.width, 1);
    assert.equal(c.height, 1);
  });
});

describe("fitSize", () => {
  it("quadrato, alto, largo", () => {
    assert.deepEqual(fitSize(100, 100, 24, 34), { w: 24, h: 24 });
    assert.deepEqual(fitSize(50, 200, 24, 34), { w: 9, h: 34 });
    assert.deepEqual(fitSize(200, 50, 24, 34), { w: 24, h: 6 });
  });
  it("ingrandisce le immagini piccole", () => {
    assert.deepEqual(fitSize(2, 2, 24, 34), { w: 24, h: 24 });
    assert.deepEqual(fitSize(1, 1, 24, 34), { w: 24, h: 24 });
  });
  it("aspetto estremo: lati >= 1", () => {
    assert.deepEqual(fitSize(1000, 10, 24, 34), { w: 24, h: 1 });
    assert.deepEqual(fitSize(10, 1000, 24, 34), { w: 1, h: 34 });
  });
});

describe("resample", () => {
  it("2x2 -> 1x1 media", () => {
    const img = make(2, 2, (x, y) => (x + y === 0 ? [200, 0, 0, 255] : [0, 0, 100, 255]));
    assert.deepEqual(px(resample(img, { w: 1, h: 1 }), 0, 0), [50, 0, 75, 255]);
  });
  it("un pixel trasparente non scurisce il colore", () => {
    const img = make(2, 1, (x) => (x === 0 ? [200, 100, 50, 255] : [0, 0, 0, 0]));
    assert.deepEqual(px(resample(img, { w: 1, h: 1 }), 0, 0), [200, 100, 50, 128]);
  });
  it("ingrandimento 1x1 -> 3x3 ripete il pixel", () => {
    const out = resample(solid(1, 1, [10, 20, 30, 255]), { w: 3, h: 3 });
    assert.equal(out.width, 3);
    assert.equal(out.height, 3);
    for (let y = 0; y < 3; y++)
      for (let x = 0; x < 3; x++) assert.deepEqual(px(out, x, y), [10, 20, 30, 255]);
  });
  it("uscita trasparente ha rgb 0 e la dimensione richiesta", () => {
    const out = resample(solid(4, 4, [9, 9, 9, 0]), { w: 2, h: 3 });
    assert.equal(out.data.length, 2 * 3 * 4);
    assert.deepEqual(px(out, 1, 2), CLEAR);
  });
  it("dimensioni non divisibili: copertura frazionaria", () => {
    const out = resample(solid(3, 3, RED), { w: 2, h: 2 });
    for (let y = 0; y < 2; y++) for (let x = 0; x < 2; x++) assert.deepEqual(px(out, x, y), RED);
  });
});

describe("thresholdAlpha", () => {
  it("127 -> trasparente, 128 -> opaco", () => {
    const img = make(2, 1, (x) => [10, 20, 30, x === 0 ? ALPHA_THRESHOLD - 1 : ALPHA_THRESHOLD]);
    const out = thresholdAlpha(img);
    assert.deepEqual(px(out, 0, 0), CLEAR);
    assert.deepEqual(px(out, 1, 0), [10, 20, 30, 255]);
  });
  it("non muta l'input", () => {
    const img = solid(1, 1, [10, 20, 30, 100]);
    const out = thresholdAlpha(img);
    assert.deepEqual(px(img, 0, 0), [10, 20, 30, 100]);
    assert.notEqual(out, img);
  });
  it("soglia personalizzata", () => {
    assert.deepEqual(px(thresholdAlpha(solid(1, 1, [1, 2, 3, 50]), 50), 0, 0), [1, 2, 3, 255]);
  });
});

describe("dominantColor", () => {
  it("vince il bucket piu' frequente", () => {
    const img = make(3, 1, (x) => (x === 0 ? [0, 0, 255, 255] : RED));
    assert.equal(dominantColor(img), "#ff0000");
  });
  it("ignora i pixel trasparenti", () => {
    const img = make(4, 1, (x) => (x === 0 ? RED : [0, 0, 255, 10]));
    assert.equal(dominantColor(img), "#ff0000");
  });
  it("media dei colori reali dentro il bucket", () => {
    const img = make(2, 1, (x) => (x === 0 ? [100, 0, 0, 255] : [110, 0, 0, 255]));
    assert.equal(dominantColor(img), "#690000");
  });
  it("pari merito: il primo visto", () => {
    const img = make(2, 1, (x) => (x === 0 ? [0, 255, 0, 255] : RED));
    assert.equal(dominantColor(img), "#00ff00");
  });
  it("null se non c'e' nulla di opaco; formato minuscolo", () => {
    assert.equal(dominantColor(solid(2, 2, [255, 255, 255, 127])), null);
    assert.equal(dominantColor(solid(1, 1, [171, 205, 239, 255])), "#abcdef");
  });
});

describe("pixelate", () => {
  it("ritaglia i margini e adatta a 24x34 mantenendo l'aspetto", () => {
    const img = make(100, 100, (x, y) => (x >= 40 && x < 60 && y >= 10 && y < 90 ? RED : CLEAR));
    const out = pixelate(img);
    assert.equal(out.image.width, 9);
    assert.equal(out.image.height, BOT_IMAGE_MAX_H);
    assert.equal(out.color, "#ff0000");
  });
  it("un'immagine larga e' limitata dalla larghezza", () => {
    const out = pixelate(solid(100, 10, RED));
    assert.equal(out.image.width, BOT_IMAGE_MAX_W);
    assert.ok(out.image.height < BOT_IMAGE_MAX_H);
  });
  it("tutto trasparente: null", () => {
    assert.equal(pixelate(solid(10, 10, CLEAR)), null);
  });
  it("un rettangolo opaco resta un rettangolo", () => {
    const out = pixelate(make(30, 30, (x, y) => (x >= 5 && x < 25 && y >= 5 && y < 25 ? RED : CLEAR)));
    for (let y = 0; y < out.image.height; y++)
      for (let x = 0; x < out.image.width; x++) assert.deepEqual(px(out.image, x, y), RED);
  });
});

describe("pngSize", () => {
  function header(w, h) {
    const b = Buffer.alloc(33);
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).copy(b, 0);
    b.writeUInt32BE(13, 8);
    b.write("IHDR", 12, "ascii");
    b.writeUInt32BE(w, 16);
    b.writeUInt32BE(h, 20);
    return b;
  }
  it("legge larghezza e altezza", () => {
    assert.deepEqual(pngSize(header(24, 34).toString("base64")), { w: 24, h: 34 });
    assert.deepEqual(pngSize(header(0xfffffff0, 1).toString("base64")), { w: 0xfffffff0, h: 1 });
  });
  it("JPEG, spazzatura o input corto: null", () => {
    const jpeg = Buffer.from([0xff, 0xd8, 0xff, 0xe0, ...new Array(40).fill(0)]);
    assert.equal(pngSize(jpeg.toString("base64")), null);
    assert.equal(pngSize("!!!not base64!!!"), null);
    assert.equal(pngSize(""), null);
    assert.equal(pngSize(header(1, 1).subarray(0, 10).toString("base64")), null);
  });
  it("IHDR mancante: null", () => {
    const b = header(5, 5);
    b.write("IDAT", 12, "ascii");
    assert.equal(pngSize(b.toString("base64")), null);
  });
});
