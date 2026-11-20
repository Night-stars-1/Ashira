//! Bounded bridge between a token-authenticated WebSocket and a native shell PTY.
use std::{io::{Read, Write}, path::PathBuf, time::Duration};

use futures_util::{SinkExt, StreamExt};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use tokio::{net::TcpStream, sync::mpsc, time};
use tokio_tungstenite::{WebSocketStream, tungstenite::{Message, protocol::{CloseFrame, Role, WebSocketConfig, frame::coding::CloseCode}}};
use tokio_util::sync::CancellationToken;

const INPUT_LIMIT: usize = 64 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

enum Input {
    Bytes(tokio_tungstenite::tungstenite::Bytes),
    Size(PtySize),
}

struct Shell {
    input: Option<mpsc::Sender<Input>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    #[cfg(windows)]
    job: Option<crate::process_guard::ProcessGuard>,
}

impl Drop for Shell {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        if let Some(mut child) = self.child.take() {
            // The POSIX shell is the PTY session leader; HUP reaches its jobs.
            #[cfg(unix)]
            if let Some(pid) = child.process_id() {
                unsafe { libc::kill(-(pid as i32), libc::SIGHUP); libc::kill(pid as i32, libc::SIGKILL); }
            }
            drop(self.input.take());
            tokio::task::spawn_blocking(move || { let _ = child.wait(); });
        }
    }
}

fn open(cwd: PathBuf, size: PtySize) -> Result<(Shell, mpsc::Receiver<Vec<u8>>), String> {
    let command = |shell: &std::ffi::OsStr| {
        let mut command = CommandBuilder::new(shell);
        command.cwd(&cwd);
        command.env("TERM", "xterm-256color");
        #[cfg(windows)]
        command.arg("-NoLogo");
        command
    };
    let pair = native_pty_system().openpty(size).map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let mut child = {
        // Do not resolve an executable relative to the selected workspace.
        let pwsh = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path).filter(|dir| dir.is_absolute())
                .map(|dir| dir.join("pwsh.exe")).find(|file| file.is_file())
        });
        match pwsh.as_deref().and_then(|file| pair.slave.spawn_command(command(file.as_os_str())).ok()) {
            Some(child) => child,
            None => {
                let root = std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable")?;
                let shell = PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
                pair.slave.spawn_command(command(shell.as_os_str())).map_err(|e| e.to_string())?
            }
        }
    };
    #[cfg(not(windows))]
    let child = {
        let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
        pair.slave.spawn_command(command(&shell)).map_err(|e| e.to_string())?
    };
    drop(pair.slave);
    #[cfg(windows)]
    let job = match child.as_raw_handle().ok_or_else(|| "Shell process handle unavailable".to_owned())
        .and_then(crate::process_guard::ProcessGuard::attach_windows) {
        Ok(job) => job,
        Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
    };
    let (output, rx) = mpsc::channel(16);
    tokio::task::spawn_blocking(move || {
        let mut buf = [0; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            if output.blocking_send(buf[..n].to_vec()).is_err() {
                // ConPTY may emit final output while its master is closing.
                while reader.read(&mut buf).is_ok_and(|n| n > 0) {}
                break;
            }
        }
    });
    let (input, mut queue) = mpsc::channel(32);
    tokio::task::spawn_blocking(move || {
        while let Some(input) = queue.blocking_recv() {
            let result = match input {
                Input::Size(size) => pair.master.resize(size).map_err(|e| e.to_string()),
                Input::Bytes(bytes) => writer.write_all(&bytes).and_then(|()| writer.flush()).map_err(|e| e.to_string()),
            };
            if result.is_err() { break; }
        }
    });
    Ok((Shell { input: Some(input), child: Some(child), #[cfg(windows)] job: Some(job) }, rx))
}

#[derive(serde::Deserialize)]
struct Size { cols: u16, rows: u16 }

fn size(text: &str) -> Option<PtySize> {
    let Size { cols, rows } = serde_json::from_str(text).ok()?;
    Some(PtySize { cols: cols.clamp(2, 400), rows: rows.clamp(1, 200), pixel_width: 0, pixel_height: 0 })
}

pub(crate) async fn run(socket: TcpStream, cwd: PathBuf, shutdown: CancellationToken) -> Result<(), String> {
    let config = WebSocketConfig::default().max_message_size(Some(INPUT_LIMIT)).max_frame_size(Some(INPUT_LIMIT));
    let mut ws = WebSocketStream::from_raw_socket(socket, Role::Server, Some(config)).await;
    let initial = tokio::select! {
        _ = shutdown.cancelled() => return Ok(()),
        value = time::timeout(Duration::from_secs(5), ws.next()) => match value {
            Ok(Some(Ok(Message::Text(text)))) => size(&text),
            _ => None,
        },
    };
    let Some(initial) = initial else { return Err("Terminal needs an initial size".into()) };
    let (mut shell, mut output) = match tokio::task::spawn_blocking(move || open(cwd, initial)).await {
        Ok(Ok(opened)) => opened,
        failure => {
            let _ = time::timeout(WRITE_TIMEOUT, ws.close(Some(CloseFrame { code: CloseCode::Error, reason: "Shell could not start".into() }))).await;
            return Err(match failure { Ok(Err(error)) => error, Err(error) => error.to_string(), _ => unreachable!() });
        }
    };
    let mut poll = time::interval(Duration::from_millis(250));
    let mut exit = None;
    let reason = loop {
        tokio::select! {
            _ = shutdown.cancelled() => break "Server stopped",
            _ = poll.tick() => {
                if let Some(at) = exit {
                    if time::Instant::now() >= at { break "Shell exited"; }
                } else if let Some(child) = shell.child.as_mut() {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            shell.child = None;
                            drop(shell.input.take());
                            exit = Some(time::Instant::now() + Duration::from_secs(1));
                        }
                        Ok(None) => {}
                        Err(_) => break "Shell stopped",
                    }
                }
            }
            data = output.recv() => {
                let Some(data) = data else { break "Shell exited" };
                if !matches!(time::timeout(WRITE_TIMEOUT, ws.send(Message::Binary(data.into()))).await, Ok(Ok(()))) { break "Connection closed"; }
            }
            frame = ws.next() => {
                let input = match frame {
                    Some(Ok(Message::Binary(data))) => Some(Input::Bytes(data)),
                    Some(Ok(Message::Text(text))) => size(&text).map(Input::Size),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break "Connection closed",
                    _ => None,
                };
                if let (Some(sender), Some(input)) = (shell.input.as_ref(), input)
                    && sender.try_send(input).is_err() { break "Terminal input unavailable"; }
            }
        }
    };
    drop(shell);
    drop(output);
    let _ = time::timeout(WRITE_TIMEOUT, ws.close(Some(CloseFrame { code: CloseCode::Normal, reason: reason.into() }))).await;
    Ok(())
}
