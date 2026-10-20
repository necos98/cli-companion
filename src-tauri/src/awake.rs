//! Impedisce lo standby del sistema mentre c'e' lavoro automatico
//! (`keep_awake` dello schedule). Windows: `SetThreadExecutionState` vale
//! per il thread che lo chiama, quindi un thread dedicato lo tiene e lo
//! rilascia su richiesta. Altrove non fa nulla.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

static TX: OnceLock<Mutex<Sender<bool>>> = OnceLock::new();

/// Prende (true) o rilascia (false) il blocco dello standby.
pub fn set(on: bool) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::spawn(move || run(rx));
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send(on);
}

fn run(rx: Receiver<bool>) {
    for on in rx {
        apply(on);
    }
    apply(false);
}

#[cfg(windows)]
fn apply(on: bool) {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
    // SAFETY: chiamata Win32 senza puntatori.
    unsafe {
        SetThreadExecutionState(flags);
    }
}

#[cfg(not(windows))]
fn apply(_on: bool) {}
