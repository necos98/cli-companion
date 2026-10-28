//! Esecuzione di processi esterni con timeout.

use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
/// Evita la finestra di console per i processi figli su Windows.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Esegue il comando con stdout/stderr su pipe (svuotate da thread) e lo
/// termina se supera `timeout`.
pub fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Output, String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{program}: avvio fallito: {e}"))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let status = match wait_with_timeout(&mut child, timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            kill_and_reap(&mut child);
            return Err(format!("{program}: timeout dopo {} s", timeout.as_secs()));
        }
        Err(e) => {
            kill_and_reap(&mut child);
            return Err(format!("{program}: attesa fallita: {e}"));
        }
    };
    Ok(Output {
        status,
        stdout: join_output(stdout, &program)?,
        stderr: join_output(stderr, &program)?,
    })
}

fn drain<R: Read + Send + 'static>(stream: Option<R>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut stream) = stream {
            // Un errore di lettura lascia l'output parziale: basta per la diagnostica.
            let _ = stream.read_to_end(&mut buf);
        }
        buf
    })
}

fn join_output(handle: JoinHandle<Vec<u8>>, program: &str) -> Result<Vec<u8>, String> {
    handle
        .join()
        .map_err(|_| format!("{program}: lettura dell'output interrotta"))
}

fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn kill_and_reap(child: &mut Child) {
    // Il processo potrebbe essere gia' terminato: l'errore e' irrilevante.
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-Command", script]);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", script]);
            cmd
        }
    }

    #[test]
    fn captures_stdout_and_status() {
        let out = output_with_timeout(&mut shell("echo ciao"), Duration::from_secs(30)).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("ciao"));
    }

    #[test]
    fn kills_on_timeout() {
        let err = output_with_timeout(
            &mut shell("Start-Sleep 30; sleep 30"),
            Duration::from_millis(500),
        )
        .unwrap_err();
        assert!(err.contains("timeout"), "{err}");
    }

    #[test]
    fn reports_missing_program() {
        let err = output_with_timeout(
            &mut Command::new("programma-inesistente-e2e"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(err.contains("programma-inesistente-e2e"), "{err}");
    }
}
