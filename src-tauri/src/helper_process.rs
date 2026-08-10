use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
enum HelperEvent {
    State {
        state: String,
    },
    Log {
        level: String,
        msg: String,
    },
    CertUntrusted {
        fingerprint: String,
        details: String,
    },
    MfaChallenge {
        message: String,
    },
    Gateways {
        list: Vec<Gateway>,
        selecting: bool,
    },
    Connected {
        ifname: String,
        addr: Option<String>,
        dns: Vec<String>,
        gateway: String,
    },
    Error {
        msg: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Gateway {
    pub address: String,
    pub name: String,
}

/// What the tunnel ended up negotiating, shown on the connected screen.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConnectionInfo {
    pub ifname: String,
    pub addr: Option<String>,
    pub dns: Vec<String>,
    pub gateway: String,
}

/// The profile the current attempt belongs to.
#[derive(Debug, Clone, Default)]
pub struct ActiveProfile {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Default)]
pub struct VpnRuntime {
    state: Arc<Mutex<String>>,
    last_fingerprint: Arc<Mutex<Option<String>>>,
    profile: Arc<Mutex<Option<ActiveProfile>>>,
    connection: Arc<Mutex<Option<ConnectionInfo>>>,
}

impl VpnRuntime {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new("disconnected".into())),
            last_fingerprint: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            connection: Arc::new(Mutex::new(None)),
        }
    }

    pub fn state(&self) -> String {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| "disconnected".into())
    }

    pub fn last_fingerprint(&self) -> Option<String> {
        self.last_fingerprint.lock().ok()?.clone()
    }

    pub fn profile(&self) -> Option<ActiveProfile> {
        self.profile.lock().ok()?.clone()
    }

    pub fn connection(&self) -> Option<ConnectionInfo> {
        self.connection.lock().ok()?.clone()
    }

    pub fn set_profile(&self, profile: Option<ActiveProfile>) {
        if let Ok(mut current) = self.profile.lock() {
            *current = profile;
        }
    }

    fn set_connection(&self, connection: Option<ConnectionInfo>) {
        if let Ok(mut current) = self.connection.lock() {
            *current = connection;
        }
    }

    fn set_state(&self, state: String) {
        if let Ok(mut current) = self.state.lock() {
            *current = state;
        }
    }

    /// Publishes a state change to the window and the tray at once, so the two
    /// can never disagree about whether a tunnel is up.
    fn publish_state(&self, app: &AppHandle, state: String) {
        if state == "disconnected" {
            self.set_connection(None);
        }
        self.set_state(state.clone());
        let profile = self.profile();
        let _ = app.emit(
            "vpn://state",
            serde_json::json!({
                "state": state,
                "profileId": profile.as_ref().map(|profile| profile.id.clone()),
            }),
        );
        crate::tray::refresh(
            app,
            &state,
            profile.as_ref().map(|profile| profile.name.as_str()),
        );
    }
}

pub struct HelperProcess {
    _stdin: ChildStdin,
    child: Child,
}

impl HelperProcess {
    pub fn spawn(app: AppHandle, runtime: VpnRuntime) -> Result<Self, String> {
        let helper = helper_path()?;
        let mut child = Command::new("pkexec")
            .arg(helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not start the privileged VPN helper: {error}"))?;
        let stdin = child.stdin.take().ok_or("Could not open helper stdin")?;
        let stdout = child.stdout.take().ok_or("Could not open helper stdout")?;
        // Anything the helper or libopenconnect writes to stderr is a
        // diagnostic. It goes to our own stderr rather than the window: the UI
        // reports failures through `vpn://error`, not through a log stream.
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        eprintln!("[gp-helper] {line}");
                    }
                }
            });
        }
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(event) = serde_json::from_str::<HelperEvent>(&line) else {
                    continue;
                };
                match event {
                    HelperEvent::State { state } => runtime.publish_state(&app, state),
                    HelperEvent::Log { level, msg } => eprintln!("[gp-helper/{level}] {msg}"),
                    HelperEvent::CertUntrusted {
                        fingerprint,
                        details,
                    } => {
                        if let Ok(mut last) = runtime.last_fingerprint.lock() {
                            *last = Some(fingerprint.clone());
                        }
                        let _ = app.emit(
                            "vpn://cert-untrusted",
                            serde_json::json!({ "fingerprint": fingerprint, "details": details }),
                        );
                    }
                    HelperEvent::MfaChallenge { message } => {
                        let _ = app.emit(
                            "vpn://mfa-challenge",
                            serde_json::json!({ "message": message }),
                        );
                    }
                    HelperEvent::Gateways { list, selecting } => {
                        let _ = app.emit(
                            "vpn://gateways",
                            serde_json::json!({ "list": list, "selecting": selecting }),
                        );
                    }
                    HelperEvent::Connected {
                        ifname,
                        addr,
                        dns,
                        gateway,
                    } => {
                        let info = ConnectionInfo {
                            ifname,
                            addr,
                            dns,
                            gateway,
                        };
                        runtime.set_connection(Some(info.clone()));
                        let _ = app.emit("vpn://connected", info);
                    }
                    HelperEvent::Error { msg } => {
                        let _ = app.emit("vpn://error", serde_json::json!({ "msg": msg }));
                    }
                }
            }
            runtime.publish_state(&app, "disconnected".into());
            runtime.set_profile(None);
            crate::tray::refresh(&app, "disconnected", None);
        });
        Ok(Self {
            _stdin: stdin,
            child,
        })
    }

    pub fn send(&mut self, command: Value) -> Result<(), String> {
        let line = serde_json::to_vec(&command).map_err(|error| error.to_string())?;
        self._stdin
            .write_all(&line)
            .map_err(|error| error.to_string())?;
        self._stdin
            .write_all(b"\n")
            .map_err(|error| error.to_string())?;
        self._stdin.flush().map_err(|error| error.to_string())
    }

    pub fn is_finished(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        let _ = self.child.try_wait();
    }
}

fn helper_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("GP_HELPER_PATH") {
        return Ok(PathBuf::from(path));
    }
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable
        .parent()
        .ok_or("Could not locate app executable directory")?;
    Ok(directory.join(if cfg!(windows) {
        "gp-helper.exe"
    } else {
        "gp-helper"
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub state: String,
    pub profile_id: Option<String>,
    pub connection: Option<ConnectionInfo>,
}
