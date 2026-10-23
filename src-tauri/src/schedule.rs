//! Fasce orarie (contratto: `docs/BOARD.MD`, "Fasce orarie"). Servono solo
//! come gate dello scheduler dei workspace che le usano.
//!
//! Parte PURA: tipi, validazione, `window_open`, `next_change`; niente I/O ne'
//! orologio (il chiamante passa i minuti locali). Aritmetica sui minuti del
//! giorno, indifferente al cambio dell'ora legale: il thread dei timer rilegge
//! l'ora locale a ogni giro. Persistenza (`schedule.json`) in fondo;
//! l'integrazione con il `Registry` e' in `automation/auto.rs`.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MAX_WINDOWS: usize = 12;
pub const MINUTES_PER_DAY: u16 = 1440;
pub const SCHEDULE_FILE: &str = "schedule.json";

/// Finestra `[start, end)` in ora locale, `HH:MM`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    pub windows: Vec<Window>,
    /// Impedisce lo standby del sistema finche' un task e' in corso.
    pub keep_awake: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { enabled: false, windows: Vec::new(), keep_awake: true }
    }
}

/// Stato esposto al frontend (comandi e evento `schedule-state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleState {
    pub schedule: Schedule,
    pub window_open: bool,
    /// Unix ms del prossimo cambio (apertura o chiusura), se esiste.
    pub next_change_at: Option<i64>,
}

/// `HH:MM` -> minuti dal mezzanotte (strict: due cifre, 00..23 e 00..59).
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let b = s.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return None;
    }
    let digit = |c: u8| c.is_ascii_digit().then(|| (c - b'0') as u16);
    let h = digit(b[0])? * 10 + digit(b[1])?;
    let m = digit(b[3])? * 10 + digit(b[4])?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Validazione del comando `schedule_set` (errori per l'utente: italiano).
pub fn validate(s: &Schedule) -> Result<(), String> {
    if s.windows.len() > MAX_WINDOWS {
        return Err(format!("troppe fasce orarie (massimo {MAX_WINDOWS})"));
    }
    for w in &s.windows {
        let (Some(a), Some(b)) = (parse_hhmm(&w.start), parse_hhmm(&w.end)) else {
            return Err(format!("orario non valido ({} - {}): usa il formato HH:MM", w.start, w.end));
        };
        if a == b {
            return Err(format!("la fascia {} - {} ha inizio e fine uguali", w.start, w.end));
        }
    }
    Ok(())
}

/// Finestre gia' parsate; quelle illeggibili (file modificato a mano) si
/// ignorano.
fn spans(s: &Schedule) -> Vec<(u16, u16)> {
    s.windows
        .iter()
        .filter_map(|w| Some((parse_hhmm(&w.start)?, parse_hhmm(&w.end)?)))
        .filter(|(a, b)| a != b)
        .collect()
}

fn covers((start, end): (u16, u16), minute: u16) -> bool {
    if start < end {
        minute >= start && minute < end
    } else {
        // Attraversa la mezzanotte: [start, 24:00) + [00:00, end).
        minute >= start || minute < end
    }
}

fn open_in(spans: &[(u16, u16)], minute: u16) -> bool {
    spans.iter().any(|s| covers(*s, minute))
}

/// Con lo schedule abilitato, `minute` (minuti locali del giorno) e' dentro
/// una finestra? Disabilitato = sempre `false`: nessun effetto.
pub fn window_open(s: &Schedule, minute: u16) -> bool {
    s.enabled && open_in(&spans(s), minute % MINUTES_PER_DAY)
}

/// Fra quanti minuti (1..=1440) cambia lo stato (apre o chiude). `None` se
/// disabilitato o se non cambia mai (nessuna finestra, o l'unione copre
/// tutto il giorno). Scansione minuto per minuto: robusta a sovrapposizioni
/// e confini.
pub fn next_change(s: &Schedule, minute: u16) -> Option<u16> {
    if !s.enabled {
        return None;
    }
    let sp = spans(s);
    let minute = minute % MINUTES_PER_DAY;
    let now = open_in(&sp, minute);
    (1..=MINUTES_PER_DAY).find(|d| open_in(&sp, (minute + d) % MINUTES_PER_DAY) != now)
}

/// Unix ms del prossimo cambio: inizio del minuto corrente + minuti mancanti.
pub fn next_change_at(s: &Schedule, minute: u16, now_ms: i64) -> Option<i64> {
    let d = next_change(s, minute)?;
    Some(now_ms - now_ms.rem_euclid(60_000) + d as i64 * 60_000)
}

pub fn state(s: &Schedule, minute: u16, now_ms: i64) -> ScheduleState {
    ScheduleState {
        schedule: s.clone(),
        window_open: window_open(s, minute),
        next_change_at: next_change_at(s, minute, now_ms),
    }
}

// ---------------------------------------------------------------------------
// Persistenza dello schedule
// ---------------------------------------------------------------------------

/// Carica lo schedule; file assente = default; corrotto o non valido va in
/// `.json.bak` e vale il default.
pub fn load_schedule(dir: &Path) -> Schedule {
    let path = dir.join(SCHEDULE_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Schedule::default();
    };
    match serde_json::from_str::<Schedule>(&raw) {
        Ok(s) if validate(&s).is_ok() => s,
        _ => {
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
            Schedule::default()
        }
    }
}

pub fn save_schedule(dir: &Path, s: &Schedule) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("impossibile creare cartella dati: {e}"))?;
    let raw = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    crate::write_atomic(&dir.join(SCHEDULE_FILE), &raw).map_err(|e| format!("impossibile salvare lo schedule: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(ws: &[(&str, &str)]) -> Schedule {
        Schedule {
            enabled: true,
            windows: ws.iter().map(|(a, b)| Window { start: a.to_string(), end: b.to_string() }).collect(),
            keep_awake: true,
        }
    }

    fn m(s: &str) -> u16 {
        parse_hhmm(s).unwrap()
    }

    #[test]
    fn hhmm_is_strict() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm("20:05"), Some(1205));
        for bad in ["", "9:00", "09:0", "24:00", "12:60", "12-30", "ab:cd", "12:300", " 12:30", "+1:30"] {
            assert_eq!(parse_hhmm(bad), None, "{bad}");
        }
    }

    #[test]
    fn validation_rules() {
        assert!(validate(&sched(&[("20:00", "13:00"), ("14:00", "15:00")])).is_ok());
        assert!(validate(&sched(&[])).is_ok());
        assert!(validate(&sched(&[("10:00", "10:00")])).is_err());
        assert!(validate(&sched(&[("10:00", "25:00")])).is_err());
        assert!(validate(&sched(&[("1:00", "05:00")])).is_err());
        let many: Vec<(&str, &str)> = (0..12).map(|_| ("01:00", "02:00")).collect();
        assert!(validate(&sched(&many)).is_ok());
        let too_many: Vec<(&str, &str)> = (0..13).map(|_| ("01:00", "02:00")).collect();
        assert!(validate(&sched(&too_many)).is_err());
    }

    #[test]
    fn same_day_window_is_half_open() {
        let s = sched(&[("09:00", "17:00")]);
        assert!(!window_open(&s, m("08:59")));
        assert!(window_open(&s, m("09:00")));
        assert!(window_open(&s, m("16:59")));
        assert!(!window_open(&s, m("17:00")));
        assert!(!window_open(&s, m("00:00")));
    }

    #[test]
    fn midnight_crossing_window() {
        let s = sched(&[("20:00", "13:00")]);
        assert!(window_open(&s, m("20:00")));
        assert!(window_open(&s, m("23:59")));
        assert!(window_open(&s, m("00:00")));
        assert!(window_open(&s, m("12:59")));
        assert!(!window_open(&s, m("13:00")));
        assert!(!window_open(&s, m("19:59")));
        // Fine esattamente a mezzanotte.
        let s = sched(&[("22:00", "00:00")]);
        assert!(window_open(&s, m("23:59")));
        assert!(!window_open(&s, m("00:00")));
        assert!(!window_open(&s, m("21:59")));
        // Inizio a mezzanotte.
        let s = sched(&[("00:00", "06:00")]);
        assert!(window_open(&s, 0) && window_open(&s, m("05:59")) && !window_open(&s, m("06:00")));
    }

    #[test]
    fn overlapping_windows_are_a_union() {
        let s = sched(&[("08:00", "12:00"), ("10:00", "14:00"), ("23:00", "02:00"), ("01:00", "03:00")]);
        for t in ["08:00", "11:59", "13:59", "23:30", "01:30", "02:59"] {
            assert!(window_open(&s, m(t)), "{t}");
        }
        for t in ["14:00", "07:59", "03:00", "22:59"] {
            assert!(!window_open(&s, m(t)), "{t}");
        }
        // Cambi: l'unione 08-14 si chiude alle 14, non alle 12.
        assert_eq!(next_change(&s, m("09:00")), Some(5 * 60));
    }

    #[test]
    fn disabled_or_empty_never_opens() {
        let mut s = sched(&[("00:00", "23:59")]);
        s.enabled = false;
        assert!(!window_open(&s, m("12:00")));
        assert_eq!(next_change(&s, m("12:00")), None);
        assert!(!window_open(&sched(&[]), m("12:00")));
        assert_eq!(next_change(&sched(&[]), m("12:00")), None);
    }

    #[test]
    fn next_change_boundaries() {
        let s = sched(&[("20:00", "13:00")]);
        // Chiusa alle 15:00: apre fra 5 ore.
        assert_eq!(next_change(&s, m("15:00")), Some(300));
        // Un minuto prima dell'apertura.
        assert_eq!(next_change(&s, m("19:59")), Some(1));
        // Aperta alle 20:00: chiude alle 13:00 del giorno dopo = 17 ore.
        assert_eq!(next_change(&s, m("20:00")), Some(17 * 60));
        // Alle 12:59 chiude fra un minuto.
        assert_eq!(next_change(&s, m("12:59")), Some(1));
        // Alla chiusura il prossimo cambio e' l'apertura delle 20:00.
        assert_eq!(next_change(&s, m("13:00")), Some(7 * 60));
        // Tutto il giorno coperto da due finestre adiacenti: mai.
        let all = sched(&[("00:00", "12:00"), ("12:00", "00:00")]);
        assert!(window_open(&all, m("12:00")) && window_open(&all, m("23:59")));
        assert_eq!(next_change(&all, m("05:00")), None);
        // Finestra di un solo minuto.
        let one = sched(&[("10:00", "10:01")]);
        assert_eq!(next_change(&one, m("10:00")), Some(1));
        assert_eq!(next_change(&one, m("10:01")), Some(1439));
    }

    #[test]
    fn next_change_at_uses_the_minute_start() {
        let s = sched(&[("20:00", "13:00")]);
        // 15:00:42.500 locale (ms arbitrari multipli di minuto + 42 500).
        let base = 1_700_000_000_000 - 1_700_000_000_000 % 60_000;
        let now = base + 42_500;
        assert_eq!(next_change_at(&s, m("15:00"), now), Some(base + 300 * 60_000));
        assert_eq!(next_change_at(&sched(&[]), m("15:00"), now), None);
        let st = state(&s, m("21:00"), now);
        assert!(st.window_open);
        assert_eq!(st.next_change_at, Some(base + 16 * 60 * 60_000));
    }

    #[test]
    fn minute_wraps_modulo_a_day() {
        let s = sched(&[("20:00", "13:00")]);
        assert_eq!(window_open(&s, 1440 + 60), window_open(&s, 60));
    }

    #[test]
    fn schedule_json_roundtrip_and_defaults() {
        let s: Schedule = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Schedule::default());
        assert!(s.keep_awake && !s.enabled);
        let raw = r#"{"enabled":true,"windows":[{"start":"20:00","end":"13:00"}],"keep_awake":false}"#;
        let s: Schedule = serde_json::from_str(raw).unwrap();
        assert_eq!(serde_json::to_string(&s).unwrap(), raw);
        let v = serde_json::to_value(state(&s, 0, 0)).unwrap();
        assert_eq!(v["window_open"], true);
        assert!(v["next_change_at"].is_i64());
        assert_eq!(v["schedule"]["windows"][0]["end"], "13:00");
    }

    #[test]
    fn files_roundtrip_in_a_temp_dir() {
        let dir = std::env::temp_dir().join(format!("cc-sched-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Schedule.
        assert_eq!(load_schedule(&dir), Schedule::default());
        let s = sched(&[("20:00", "13:00")]);
        save_schedule(&dir, &s).unwrap();
        assert_eq!(load_schedule(&dir), s);
        // File non valido: .bak e default.
        std::fs::write(dir.join(SCHEDULE_FILE), r#"{"enabled":true,"windows":[{"start":"1:00","end":"2:00"}]}"#).unwrap();
        assert_eq!(load_schedule(&dir), Schedule::default());
        assert!(dir.join("schedule.json.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
