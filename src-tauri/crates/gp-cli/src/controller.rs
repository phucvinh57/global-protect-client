//! An unprivileged session owner. Only the JSON-lines tunnel helper uses sudo.
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    process::Command,
    sync::{mpsc, oneshot},
};

const SYSTEM_LOCK: &str = "/run/lock/gp-client-tunnel.lock";
const SOCKET: &str = "control.sock";

fn uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn private_directory(path: &Path) -> Result<(), String> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != uid() || meta.mode() & 0o077 != 0 {
        return Err(format!(
            "Unsafe runtime directory: {} (expected an owned directory with mode 0700)",
            path.display()
        ));
    }
    Ok(())
}

fn runtime_dir() -> Result<PathBuf, String> {
    let base = match std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty()) {
        Some(p) => {
            let p = PathBuf::from(p);
            if !p.is_absolute() {
                return Err("XDG_RUNTIME_DIR must be absolute".into());
            }
            let m = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
            if !m.is_dir() || m.uid() != uid() || m.mode() & 0o077 != 0 {
                return Err("XDG_RUNTIME_DIR must be private and owned by this user".into());
            }
            p.join("gp-client")
        }
        None => PathBuf::from(format!("/tmp/gp-client-{}", uid())),
    };
    private_directory(&base)?;
    Ok(base)
}

fn lock(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != uid() || m.mode() & 0o077 != 0 || m.nlink() != 1 {
        return Err("Unsafe controller lock file".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another CLI connection is active or starting".into());
    }
    Ok(file)
}

fn external_tunnel() -> Result<bool, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(SYSTEM_LOCK)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("Cannot inspect tunnel ownership: {e}")),
    };
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != 0 || m.nlink() != 1 {
        return Err("Unsafe system tunnel lock".into());
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
        return Ok(false);
    }
    let e = io::Error::last_os_error();
    if e.kind() == io::ErrorKind::WouldBlock {
        Ok(true)
    } else {
        Err(e.to_string())
    }
}

pub fn ensure_available() -> Result<(), String> {
    let dir = runtime_dir()?;
    let _lock = lock(&dir.join("controller.lock"))?;
    if external_tunnel()? {
        return Err("Another instance owns the tunnel (desktop or another user)".into());
    }
    Ok(())
}

pub fn helper_path() -> Result<PathBuf, String> {
    let path = if let Some(path) = std::env::var_os("GP_HELPER_PATH") {
        PathBuf::from(path)
    } else {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let adjacent = exe
            .parent()
            .ok_or("Missing executable directory")?
            .join("gp-helper");
        if adjacent.is_file() {
            adjacent
        } else {
            PathBuf::from("/usr/local/libexec/gp-client/gp-helper")
        }
    };
    let path = fs::canonicalize(path)
        .map_err(|e| format!("Cannot find gp-helper; install it or set GP_HELPER_PATH: {e}"))?;
    Ok(path)
}

fn check_socket(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.file_type().is_socket() || m.uid() != uid() || m.mode() & 0o077 != 0 {
        return Err("Unsafe control socket".into());
    }
    Ok(())
}

pub async fn request(cmd: &str) -> Result<Value, String> {
    let dir = runtime_dir()?;
    let socket = dir.join(SOCKET);
    let connection = match fs::symlink_metadata(&socket) {
        Ok(_) => {
            check_socket(&socket)?;
            Some(UnixStream::connect(&socket).await)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let mut stream = match connection {
        Some(Ok(s)) => s,
        Some(Err(e))
            if !matches!(
                e.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            return Err(e.to_string())
        }
        _ => {
            // The lock, rather than a PID or a socket's existence, determines
            // whether a controller is alive. Never unlink a running socket.
            let _lock = lock(&dir.join("controller.lock"))?;
            if socket.exists() {
                check_socket(&socket)?;
                fs::remove_file(&socket).map_err(|e| e.to_string())?;
            }
            if external_tunnel()? {
                return if cmd == "status" {
                    Ok(json!({"state":"owned_by_another_instance"}))
                } else {
                    Err("Another instance owns the tunnel; CLI cannot disconnect it".into())
                };
            }
            return Ok(json!({"state":"disconnected"}));
        }
    };
    if stream.peer_cred().map_err(|e| e.to_string())?.uid() != uid() {
        return Err("Control socket belongs to a different user".into());
    }
    write_json(&mut stream, &json!({"cmd":cmd})).await?;
    let mut line = String::new();
    if BufReader::new(stream)
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("Controller closed before cleanup completed".into());
    }
    let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if let Some(error) = value["error"].as_str() {
        return Err(error.into());
    }
    Ok(value)
}

async fn write_json(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &Value,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await.map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
struct Control {
    command: String,
    reply: oneshot::Sender<Value>,
}

async fn receive_control(stream: UnixStream, sender: mpsc::Sender<Control>) {
    if stream.peer_cred().map(|c| c.uid() != uid()).unwrap_or(true) {
        return;
    }
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut line),
    )
    .await;
    if !matches!(read, Ok(Ok(1..))) || line.len() > 4096 {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(&line) else {
        return;
    };
    let (reply, response) = oneshot::channel();
    if sender
        .send(Control {
            command: value["cmd"].as_str().unwrap_or("").into(),
            reply,
        })
        .await
        .is_err()
    {
        return;
    }
    if let Ok(response) = response.await {
        let _ = write_json(&mut writer, &response).await;
    }
}

pub async fn run() -> Result<(), String> {
    let result = run_session().await;
    if let Err(error) = &result {
        let _ = write_json(
            &mut tokio::io::stdout(),
            &json!({"ev":"error", "msg":error}),
        )
        .await;
    }
    result
}

async fn run_session() -> Result<(), String> {
    let dir = runtime_dir()?;
    let _lock = lock(&dir.join("controller.lock"))?;
    let socket = dir.join(SOCKET);
    if socket.exists() {
        check_socket(&socket)?;
        fs::remove_file(&socket).map_err(|e| e.to_string())?;
    }
    // Binding occurs only under the lifetime lock in an owner-only directory.
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    let guard = SocketGuard(socket.clone());
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    // Async pipe input can be cancelled without leaving Tokio's blocking
    // stdin thread waiting forever when a different terminal disconnects.
    let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let pipe = tokio::net::unix::pipe::Receiver::from_owned_fd(owned)
        .map_err(|e| format!("Controller requires piped input: {e}"))?;
    let mut parent = BufReader::new(pipe).lines();
    let line = tokio::time::timeout(std::time::Duration::from_secs(15), parent.next_line())
        .await
        .map_err(|_| "Initiating terminal did not supply a request")?
        .map_err(|e| e.to_string())?
        .ok_or("Initiating terminal closed")?;
    let mut initial: Value =
        serde_json::from_str(&line).map_err(|_| "Invalid controller request")?;
    let helper_path = initial["helper"].as_str().ok_or("Missing helper path")?;
    let mut command = if uid() == 0 {
        Command::new(helper_path)
    } else {
        let mut c = Command::new("sudo");
        c.args(["-n", "--", helper_path]);
        c
    };
    let mut helper = command
        .arg("--detach")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start helper: {e}"))?;
    // sudo has exec'd in the initiating terminal's session and can use its
    // tty-scoped authorization. The helper forks out of sudo before serving
    // JSON lines. Neither process is tied to that terminal afterward.
    if unsafe { libc::setsid() } < 0 {
        return Err(format!(
            "Cannot detach controller: {}",
            io::Error::last_os_error()
        ));
    }
    let mut input = helper.stdin.take().ok_or("Missing helper stdin")?;
    let mut events = BufReader::new(helper.stdout.take().ok_or("Missing helper stdout")?).lines();
    let mut output = tokio::io::stdout();
    let mut status = json!({"state":"authenticating", "profile_id":initial["profile_id"], "profile_name":initial["profile_name"]});
    write_json(&mut input, &initial["request"]).await?;
    initial.take();
    let (sender, mut controls) = mpsc::channel::<Control>(32);
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| e.to_string())?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        .map_err(|e| e.to_string())?;
    let mut connected = false;
    let mut helper_finished = false;
    let mut parent_open = true;
    let mut pending_disconnect = Vec::new();
    let mut clients = tokio::task::JoinSet::new();
    let mut failure = None;
    loop {
        tokio::select! {
            biased;
            line = parent.next_line(), if parent_open => {
                match line {
                    Ok(Some(line)) => {
                        let answer = serde_json::from_str::<Value>(&line).ok();
                        if let Some(answer) = answer.filter(|v| matches!(v["cmd"].as_str(), Some("trust_cert" | "reject_cert" | "submit_otp" | "select_gateway"))) {
                            if let Err(e) = write_json(&mut input, &answer).await { failure = Some(e); break; }
                        } else { failure = Some("Invalid authentication answer".into()); break; }
                    }
                    _ => { parent_open = false; if !connected { failure = Some("Initiating terminal closed before connection succeeded".into()); break; } }
                }
            }
            _ = term.recv() => break,
            _ = interrupt.recv() => break,
            _ = hangup.recv() => { if !connected { break; } }
            _ = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => { clients.spawn(receive_control(stream, sender.clone())); }
                    Err(e) => { failure = Some(e.to_string()); break; }
                }
            }
            Some(control) = controls.recv() => {
                match control.command.as_str() {
                    "status" => { let _ = control.reply.send(status.clone()); }
                    "disconnect" => { pending_disconnect.push(control.reply); break; }
                    _ => { let _ = control.reply.send(json!({"error":"Unknown control command"})); }
                }
            }
            event = events.next_line() => {
                let event = match event {
                    Ok(Some(line)) => match serde_json::from_str::<Value>(&line) { Ok(v) => v, Err(_) => { failure = Some("Invalid helper event".into()); break; } },
                    Ok(None) => { failure = Some("VPN helper exited unexpectedly".into()); break; }
                    Err(e) => { failure = Some(e.to_string()); break; }
                };
                match event["ev"].as_str() {
                    Some("log") => continue, // Diagnostic output may contain protocol data.
                    Some("state") => {
                        status["state"] = event["state"].clone();
                        if event["state"] == "disconnected" { helper_finished = true; break; }
                    }
                    Some("connected") => { status["connection"] = event.clone(); status["state"] = json!("connected"); }
                    _ => {}
                }
                if !connected {
                    if let Err(e) = write_json(&mut output, &event).await { failure = Some(e); break; }
                    if event["ev"] == "connected" { connected = true; }
                }
                if event["ev"] == "error" { failure = Some(event["msg"].as_str().unwrap_or("Helper failed").to_owned()); break; }
            }
        }
    }
    // EOF and Disconnect both ask the helper to join its worker. Waiting for
    // process exit also waits for libopenconnect's routing/DNS cleanup.
    let _ = write_json(&mut input, &json!({"cmd":"disconnect"})).await;
    drop(input);
    // Drain stdout so verbose cleanup cannot block a helper with a full pipe.
    while let Ok(Some(line)) = events.next_line().await {
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            if event["ev"] == "state" && event["state"] == "disconnected" {
                helper_finished = true;
            }
        }
    }
    let exit = helper.wait().await.map_err(|e| e.to_string())?;
    drop(listener);
    drop(guard);
    let response = if exit.success() && helper_finished {
        json!({"state":"disconnected"})
    } else {
        json!({"error":"VPN helper failed during cleanup"})
    };
    for reply in pending_disconnect {
        let _ = reply.send(response.clone());
    }
    controls.close();
    while let Some(control) = controls.recv().await {
        let _ = control.reply.send(response.clone());
    }
    // Every disconnect acknowledgment must reach its client before runtime exit.
    while clients.join_next().await.is_some() {}
    if let Some(error) = failure {
        return Err(error);
    }
    if !connected {
        return Err("Connection cancelled before tunnel became ready".into());
    }
    if !exit.success() || !helper_finished {
        return Err("VPN helper exited unsuccessfully".into());
    }
    Ok(())
}
