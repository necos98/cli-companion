//! Memoria di un processo figlio.

use std::process::Child;

/// Picco del working set del processo (byte). Solo Windows, altrove `None`.
#[cfg(windows)]
pub fn peak_working_set_bytes(child: &Child) -> Option<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    // SAFETY: l'handle appartiene a `child` (vivo per tutta la chiamata) e
    // `counters` e' una struttura valida con `cb` impostato.
    let ok = unsafe { K32GetProcessMemoryInfo(child.as_raw_handle(), &mut counters, counters.cb) };
    (ok != 0).then_some(counters.PeakWorkingSetSize as u64)
}

#[cfg(not(windows))]
pub fn peak_working_set_bytes(_child: &Child) -> Option<u64> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn reports_a_plausible_peak_for_a_live_child() {
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Sleep 5"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let peak = peak_working_set_bytes(&child);
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(peak.unwrap() > 1024 * 1024);
    }
}
