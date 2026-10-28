//! Campionamento della VRAM usata (GPU 0) tramite `nvidia-smi`.

use crate::proc::output_with_timeout;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const NVIDIA_SMI: &str = "nvidia-smi";
const QUERY_ARGS: [&str; 2] = ["--query-gpu=memory.used", "--format=csv,noheader,nounits"];
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(10);
/// Granularita' con cui il thread controlla il flag di stop.
const STOP_POLL: Duration = Duration::from_millis(50);

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct VramStats {
    pub baseline_mib: Option<u64>,
    pub peak_mib: Option<u64>,
    pub samples: u32,
    pub error: Option<String>,
}

pub struct VramSampler {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<VramStats>,
}

impl VramSampler {
    /// Il primo campione e' la baseline; poi un campione ogni `interval`.
    pub fn start(interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || sample_until_stopped(&flag, interval));
        VramSampler { stop, handle }
    }

    pub fn stop(self) -> VramStats {
        self.stop.store(true, Ordering::SeqCst);
        self.handle.join().unwrap_or_else(|_| VramStats {
            error: Some("thread di campionamento VRAM terminato in panico".into()),
            ..VramStats::default()
        })
    }
}

fn sample_until_stopped(stop: &AtomicBool, interval: Duration) -> VramStats {
    let mut stats = VramStats::default();
    loop {
        record(&mut stats, read_used_mib());
        if sleep_unless_stopped(stop, interval) {
            return stats;
        }
    }
}

/// Dorme `interval` a piccoli passi; `true` se e' arrivato lo stop.
fn sleep_unless_stopped(stop: &AtomicBool, interval: Duration) -> bool {
    let deadline = Instant::now() + interval;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        thread::sleep(STOP_POLL.min(interval));
    }
    stop.load(Ordering::SeqCst)
}

fn record(stats: &mut VramStats, sample: Result<u64, String>) {
    match sample {
        Ok(mib) => {
            stats.samples += 1;
            stats.baseline_mib.get_or_insert(mib);
            stats.peak_mib = Some(stats.peak_mib.map_or(mib, |peak| peak.max(mib)));
        }
        Err(e) => {
            stats.error.get_or_insert(e);
        }
    }
}

fn read_used_mib() -> Result<u64, String> {
    let out = output_with_timeout(Command::new(NVIDIA_SMI).args(QUERY_ARGS), SAMPLE_TIMEOUT)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{NVIDIA_SMI}: {}: {}", out.status, stderr.trim()));
    }
    parse_first_gpu(&String::from_utf8_lossy(&out.stdout))
}

/// Legge la riga della GPU 0 dall'output csv (una riga per GPU).
fn parse_first_gpu(output: &str) -> Result<u64, String> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| format!("{NVIDIA_SMI}: output vuoto"))?;
    line.parse::<u64>()
        .map_err(|e| format!("{NVIDIA_SMI}: valore non valido '{line}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_and_multi_gpu_output() {
        assert_eq!(parse_first_gpu("3345\n"), Ok(3345));
        assert_eq!(parse_first_gpu("812\r\n4000\r\n"), Ok(812));
    }

    #[test]
    fn rejects_bad_output() {
        assert!(parse_first_gpu("").is_err());
        assert!(parse_first_gpu("[N/A]").is_err());
    }

    #[test]
    fn record_tracks_baseline_peak_and_first_error() {
        let mut stats = VramStats::default();
        record(&mut stats, Ok(1000));
        record(&mut stats, Ok(3000));
        record(&mut stats, Err("primo".into()));
        record(&mut stats, Err("secondo".into()));
        record(&mut stats, Ok(2000));
        assert_eq!(stats.baseline_mib, Some(1000));
        assert_eq!(stats.peak_mib, Some(3000));
        assert_eq!(stats.samples, 3);
        assert_eq!(stats.error.as_deref(), Some("primo"));
    }

    #[test]
    fn sampler_stops_promptly() {
        let started = Instant::now();
        let stats = VramSampler::start(Duration::from_secs(30)).stop();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(stats.samples > 0 || stats.error.is_some());
    }
}
