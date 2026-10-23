import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  MAX_WINDOWS,
  addWindow,
  crossesMidnight,
  formatChange,
  formatClock,
  isValidTime,
  normalizeSchedule,
  normalizeTime,
  parseTime,
  removeWindow,
  sameSchedule,
  scheduleError,
  scheduleStatusText,
  updateWindow,
  windowDescription,
  windowError,
} from "../src/lib/schedule.ts";

const at = (d, h, m = 0) => new Date(2026, 5, d, h, m).getTime();
const sched = (extra = {}) => ({
  enabled: true,
  windows: [{ start: "20:00", end: "13:00" }],
  keep_awake: true,
  ...extra,
});

describe("schedule: orari", () => {
  it("parseTime e isValidTime", () => {
    assert.equal(parseTime("00:00"), 0);
    assert.equal(parseTime("9:05"), 545);
    assert.equal(parseTime("23:59"), 1439);
    for (const bad of ["24:00", "12:60", "", "abc", "1200", "12:5"]) {
      assert.equal(isValidTime(bad), false, bad);
    }
  });
  it("normalizeTime porta a HH:MM", () => {
    assert.equal(normalizeTime(" 9:05 "), "09:05");
    assert.equal(normalizeTime("xx"), "xx");
  });
});

describe("schedule: fasce", () => {
  it("start == end non e' ammesso", () => {
    assert.match(windowError({ start: "10:00", end: "10:00" }), /diversi/);
    assert.equal(windowError({ start: "10:00", end: "11:00" }), null);
    assert.match(windowError({ start: "25:00", end: "11:00" }), /non valido/);
  });
  it("attraversa la mezzanotte", () => {
    assert.equal(crossesMidnight({ start: "20:00", end: "13:00" }), true);
    assert.equal(crossesMidnight({ start: "08:00", end: "13:00" }), false);
    assert.match(windowDescription({ start: "20:00", end: "13:00" }), /giorno dopo/);
    assert.equal(windowDescription({ start: "8:00", end: "13:00" }), "08:00 – 13:00");
  });
  it("aggiungi, aggiorna, rimuovi; massimo 12", () => {
    let w = [];
    for (let i = 0; i < 20; i++) w = addWindow(w);
    assert.equal(w.length, MAX_WINDOWS);
    w = updateWindow(w, 0, { start: "01:00" });
    assert.equal(w[0].start, "01:00");
    assert.equal(w[1].start, "20:00");
    assert.equal(removeWindow(w, 0).length, MAX_WINDOWS - 1);
  });
  it("scheduleError", () => {
    assert.equal(scheduleError(sched()), null);
    assert.match(scheduleError(sched({ windows: [] })), /almeno una/);
    assert.equal(scheduleError(sched({ enabled: false, windows: [] })), null);
    assert.match(
      scheduleError(sched({ windows: [{ start: "20:00", end: "20:00" }] })),
      /Fascia 1/,
    );
    assert.match(
      scheduleError(sched({ windows: Array(13).fill({ start: "01:00", end: "02:00" }) })),
      /massimo/i,
    );
  });
  it("normalizeSchedule e sameSchedule", () => {
    const n = normalizeSchedule(sched({ windows: [{ start: "9:00", end: "13:00" }] }));
    assert.deepEqual(n.windows, [{ start: "09:00", end: "13:00" }]);
    assert.equal(sameSchedule(sched({ windows: [{ start: "9:00", end: "13:00" }] }), n), true);
    assert.equal(sameSchedule(sched(), sched({ keep_awake: false })), false);
  });
});

describe("schedule: testi di stato", () => {
  const now = at(10, 15);
  it("formatClock e formatChange", () => {
    assert.equal(formatClock(at(10, 9, 5)), "09:05");
    assert.equal(formatChange(at(10, 20), now), "20:00");
    assert.equal(formatChange(at(11, 13), now), "13:00 di domani");
    assert.equal(formatChange(at(15, 13), now), "13:00 del 15/06");
  });
  it("disattivata", () => {
    assert.equal(scheduleStatusText(null), "Esecuzione automatica disattivata");
    assert.equal(
      scheduleStatusText({ schedule: sched({ enabled: false }), window_open: true, next_change_at: 1 }),
      "Esecuzione automatica disattivata",
    );
  });
  it("finestra aperta / prossima apertura", () => {
    assert.equal(
      scheduleStatusText({ schedule: sched(), window_open: true, next_change_at: at(10, 20) }, now),
      "Finestra aperta fino alle 20:00",
    );
    assert.equal(
      scheduleStatusText({ schedule: sched(), window_open: false, next_change_at: at(10, 20) }, now),
      "Prossima apertura alle 20:00",
    );
    assert.equal(
      scheduleStatusText({ schedule: sched(), window_open: true, next_change_at: null }, now),
      "Finestra aperta",
    );
    assert.equal(
      scheduleStatusText({ schedule: sched(), window_open: false, next_change_at: null }, now),
      "Nessuna finestra in programma",
    );
  });
});
