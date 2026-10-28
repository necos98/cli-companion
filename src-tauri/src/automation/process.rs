//! Esecuzione dei comandi dell'utente con timeout e annullamento: output
//! limitato in memoria e terminazione dell'INTERO albero di processi (cmd ->
//! npx -> node -> browser; in WSL anche i processi Linux, che `wsl.exe` non
//! porta con se' quando viene ucciso).

use super::hooks::tail;
use super::worktrees::{output_with_timeout, GitOut};
use crate::board::strip_ansi;
use super::*;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Instant;

/// Ultimi byte tenuti di ciascuno stream (stdout, stderr).
pub(super) const OUTPUT_KEEP_BYTES: usize = 64 * 1024;
/// Coda dell'output aggiunta ai messaggi di timeout/annullamento.
const ERROR_TAIL_CHARS: usize = 2000;
/// Intervallo con cui si controlla uscita, timeout e annullamento.
const POLL: Duration = Duration::from_millis(50);
/// Attesa massima dei thread che svuotano le pipe dopo la fine del processo
/// (un nipote sopravvissuto puo' tenerle aperte).
const DRAIN_WAIT: Duration = Duration::from_secs(3);
/// Attesa massima di `taskkill` e dello script di kill nella distro.
const KILL_WAIT: Duration = Duration::from_secs(10);
/// Variabile d'ambiente che marca tutti i processi di una esecuzione in WSL.
pub(super) const RUN_MARKER_VAR: &str = "CLI_COMPANION_RUN";

/// Segnale di annullamento condiviso fra chi avvia un comando e chi lo ferma.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Come si uccide l'albero di processi di un comando.
pub(super) enum KillTree {
    /// Processo nativo: `taskkill /T` (Windows) o `kill` del figlio.
    Native,
    /// Comando in una distro WSL: prima i processi Linux marcati con
    /// `marker`, poi l'albero di `wsl.exe`.
    Wsl { distro: String, marker: String },
}

/// Marcatore unico di un'esecuzione (solo `[0-9a-z-]`).
pub(super) fn new_run_marker() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Script eseguito nella distro: uccide ogni processo il cui ambiente ha
/// esattamente `CLI_COMPANION_RUN=$1`. Il marcatore e' un argomento, mai
/// interpolato nello script.
const WSL_KILL_SCRIPT: &str = "for i in 1 2; do for p in /proc/[0-9]*; do \
tr '\\0' '\\n' < \"$p/environ\" 2>/dev/null | grep -qxF \"CLI_COMPANION_RUN=$1\" && kill -KILL \"${p#/proc/}\" 2>/dev/null; \
done; done; true";

fn wsl_kill_command(distro: &str, marker: &str) -> Command {
    crate::wsl::wsl_run_command(distro, "/", &["sh", "-c", WSL_KILL_SCRIPT, "sh", marker])
}

/// Ultimi `OUTPUT_KEEP_BYTES` di uno stream, letto da un thread.
struct Drain {
    buf: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Drain {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (tx, done) = mpsc::channel();
    let shared = buf.clone();
    std::thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut b = shared.lock().unwrap();
                b.extend_from_slice(&chunk[..n]);
                // Si taglia a blocchi: memoria limitata a ~2x il tetto.
                if b.len() > 2 * OUTPUT_KEEP_BYTES {
                    let cut = b.len() - OUTPUT_KEEP_BYTES;
                    b.drain(..cut);
                }
            }
        }
        let _ = tx.send(());
    });
    Drain { buf, done }
}

impl Drain {
    /// Aspetta la fine della lettura fino a `deadline`, poi ritorna quanto
    /// raccolto (lossy: un taglio dentro una sequenza UTF-8 non fa panico).
    fn collect(&self, deadline: Instant) -> String {
        let _ = self.done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        let b = self.buf.lock().unwrap();
        String::from_utf8_lossy(&b[b.len().saturating_sub(OUTPUT_KEEP_BYTES)..]).into_owned()
    }
}

/// `taskkill /PID <pid> /T /F` con attesa limitata. Solo Windows.
#[cfg(windows)]
fn taskkill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let spawned = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn();
    if let Ok(mut tk) = spawned {
        let start = Instant::now();
        while matches!(tk.try_wait(), Ok(None)) {
            if start.elapsed() > KILL_WAIT {
                let _ = tk.kill();
                let _ = tk.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn kill_tree(child: &mut Child, how: &KillTree) {
    if let KillTree::Wsl { distro, marker } = how {
        let _ = output_with_timeout(wsl_kill_command(distro, marker), KILL_WAIT);
    }
    #[cfg(windows)]
    taskkill_tree(child.id());
    // Rete di sicurezza (e unico meccanismo fuori da Windows).
    let _ = child.kill();
    let _ = child.wait();
}

/// Ultimi caratteri dell'output, da accodare a un errore (vuoto se non c'e').
fn error_with_tail(msg: String, out: &GitOut) -> String {
    let t = tail(&strip_ansi(&format!("{}\n{}", out.stdout.trim(), out.stderr.trim())), ERROR_TAIL_CHARS);
    if t.is_empty() {
        msg
    } else {
        format!("{msg}\n--- last output ---\n{t}")
    }
}

/// Esegue `cmd` (stdin nullo, output catturato: solo gli ultimi
/// `OUTPUT_KEEP_BYTES` per stream) fino a uscita, `timeout` o `cancel`; in
/// questi ultimi due casi uccide l'albero di processi e ritorna `Err`
/// ("timed out after N s" / "cancelled", con la coda dell'output).
pub(super) fn run_command(
    mut cmd: Command,
    timeout: Duration,
    cancel: Option<&CancelFlag>,
    how: &KillTree,
) -> Result<GitOut, String> {
    let cancelled = || cancel.is_some_and(CancelFlag::is_cancelled);
    if cancelled() {
        return Err("cancelled".to_string());
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let start = Instant::now();
    let finish = |ok: bool| {
        let deadline = Instant::now() + DRAIN_WAIT;
        GitOut { ok, stdout: out.collect(deadline), stderr: err.collect(deadline) }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(finish(status.success())),
            Ok(None) => {}
            Err(e) => {
                kill_tree(&mut child, how);
                return Err(e.to_string());
            }
        }
        let why = if cancelled() {
            "cancelled".to_string()
        } else if start.elapsed() >= timeout {
            format!("timed out after {} s", timeout.as_secs())
        } else {
            std::thread::sleep(POLL);
            continue;
        };
        kill_tree(&mut child, how);
        return Err(error_with_tail(why, &finish(false)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_kill_passes_the_marker_as_a_separate_argv_item() {
        let marker = "12-3-456";
        let c = wsl_kill_command("Ubuntu", marker);
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[..5], ["-d", "Ubuntu", "--cd", "/", "-e"]);
        assert_eq!(&args[5..], ["sh", "-c", WSL_KILL_SCRIPT, "sh", marker]);
        assert!(!WSL_KILL_SCRIPT.contains(marker));
    }

    #[test]
    fn run_markers_are_unique_and_safe() {
        let (a, b) = (new_run_marker(), new_run_marker());
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase() || c == '-'));
    }

    #[test]
    fn cancel_flag_is_shared_between_clones() {
        let a = CancelFlag::default();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
    }

    #[cfg(windows)]
    mod windows {
        use super::*;

        fn cmd_c(script: &str) -> Command {
            use std::os::windows::process::CommandExt;
            let mut c = Command::new("cmd.exe");
            c.raw_arg(format!("/D /S /C \"{script}\"")).creation_flags(0x08000000).current_dir(std::env::temp_dir());
            c
        }

        fn run(script: &str, timeout_s: u64, cancel: Option<&CancelFlag>) -> Result<GitOut, String> {
            run_command(cmd_c(script), Duration::from_secs(timeout_s), cancel, &KillTree::Native)
        }

        fn cancel_after(flag: &CancelFlag, ms: u64) {
            let f = flag.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                f.cancel();
            });
        }

        #[test]
        fn exit_codes_and_output() {
            let o = run("echo hello", 20, None).unwrap();
            assert!(o.ok && o.stdout.contains("hello"), "{o:?}");
            let o = run("echo oops 1>&2 && exit 3", 20, None).unwrap();
            assert!(!o.ok && o.stderr.contains("oops"), "{o:?}");
        }

        #[test]
        fn output_is_bounded_and_keeps_the_end() {
            let o = run("for /L %i in (1,1,20000) do @echo line%i", 60, None).unwrap();
            assert!(o.ok);
            assert!(o.stdout.len() <= OUTPUT_KEEP_BYTES, "{}", o.stdout.len());
            assert!(o.stdout.contains("line20000"));
        }

        #[test]
        fn cancel_kills_the_command() {
            let flag = CancelFlag::default();
            cancel_after(&flag, 300);
            let t = Instant::now();
            let e = run("ping -n 60 127.0.0.1", 120, Some(&flag)).unwrap_err();
            assert!(e.starts_with("cancelled"), "{e}");
            assert!(t.elapsed() < Duration::from_secs(10));
        }

        #[test]
        fn already_cancelled_does_not_spawn() {
            let flag = CancelFlag::default();
            flag.cancel();
            let marker = std::env::temp_dir().join(format!("cc-proc-{}.txt", new_run_marker()));
            let e = run(&format!("echo x> {}", marker.display()), 20, Some(&flag)).unwrap_err();
            assert_eq!(e, "cancelled");
            assert!(!marker.exists());
        }

        #[test]
        fn timeout_kills_the_command() {
            let t = Instant::now();
            let e = run("echo before && ping -n 60 127.0.0.1", 1, None).unwrap_err();
            assert!(e.starts_with("timed out after 1 s"), "{e}");
            assert!(e.contains("--- last output ---") && e.contains("before"), "{e}");
            assert!(t.elapsed() < Duration::from_secs(10));
        }

        #[test]
        fn grandchildren_are_killed_with_the_tree() {
            let flag = CancelFlag::default();
            cancel_after(&flag, 500);
            let t = Instant::now();
            let e = run("cmd /c ping -n 60 127.0.0.1", 120, Some(&flag)).unwrap_err();
            assert!(e.starts_with("cancelled"), "{e}");
            assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
        }
    }
}
