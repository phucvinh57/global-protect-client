use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
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
    Stats {
        rx_bytes: u64,
        tx_bytes: u64,
        rx_packets: u64,
        tx_packets: u64,
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

#[derive(Debug, Clone, Copy, Default)]
struct RawCounters {
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
}

impl RawCounters {
    fn delta(self, previous: Option<Self>) -> Self {
        let delta = |current, before| {
            if current >= before {
                current - before
            } else {
                current
            }
        };
        match previous {
            Some(previous) => Self {
                rx_bytes: delta(self.rx_bytes, previous.rx_bytes),
                tx_bytes: delta(self.tx_bytes, previous.tx_bytes),
                rx_packets: delta(self.rx_packets, previous.rx_packets),
                tx_packets: delta(self.tx_packets, previous.tx_packets),
            },
            None => self,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            rx_bytes: self.rx_bytes.saturating_add(other.rx_bytes),
            tx_bytes: self.tx_bytes.saturating_add(other.tx_bytes),
            rx_packets: self.rx_packets.saturating_add(other.rx_packets),
            tx_packets: self.tx_packets.saturating_add(other.tx_packets),
        }
    }
}

impl From<crate::settings::NetworkTotals> for RawCounters {
    fn from(value: crate::settings::NetworkTotals) -> Self {
        Self {
            rx_bytes: value.rx_bytes,
            tx_bytes: value.tx_bytes,
            rx_packets: value.rx_packets,
            tx_packets: value.tx_packets,
        }
    }
}

impl From<RawCounters> for crate::settings::NetworkTotals {
    fn from(value: RawCounters) -> Self {
        Self {
            rx_bytes: value.rx_bytes,
            tx_bytes: value.tx_bytes,
            rx_packets: value.rx_packets,
            tx_packets: value.tx_packets,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCounters {
    download_bytes: String,
    upload_bytes: String,
    download_packets: String,
    upload_packets: String,
}

impl From<RawCounters> for NetworkCounters {
    fn from(value: RawCounters) -> Self {
        Self {
            download_bytes: value.rx_bytes.to_string(),
            upload_bytes: value.tx_bytes.to_string(),
            download_packets: value.rx_packets.to_string(),
            upload_packets: value.tx_packets.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    download_bytes_per_second: f64,
    upload_bytes_per_second: f64,
    session: NetworkCounters,
    lifetime: NetworkCounters,
}

struct StatsTracker {
    profile_id: String,
    baseline: RawCounters,
    session: RawCounters,
    previous: Option<RawCounters>,
    previous_at: Option<Instant>,
    last_checkpoint: Instant,
    latest: NetworkStats,
}

impl StatsTracker {
    fn new(profile_id: String, baseline: crate::settings::NetworkTotals) -> Self {
        let baseline = RawCounters::from(baseline);
        Self {
            profile_id,
            baseline,
            session: RawCounters::default(),
            previous: None,
            previous_at: None,
            last_checkpoint: Instant::now(),
            latest: NetworkStats {
                download_bytes_per_second: 0.0,
                upload_bytes_per_second: 0.0,
                session: RawCounters::default().into(),
                lifetime: baseline.into(),
            },
        }
    }

    fn record(&mut self, raw: RawCounters) -> (NetworkStats, bool) {
        let now = Instant::now();
        let delta = raw.delta(self.previous);
        let seconds = self
            .previous_at
            .map(|previous| now.duration_since(previous).as_secs_f64())
            .unwrap_or(0.0);
        self.session = self.session.add(delta);
        let lifetime = self.baseline.add(self.session);
        self.latest = NetworkStats {
            download_bytes_per_second: if seconds > 0.0 {
                delta.rx_bytes as f64 / seconds
            } else {
                0.0
            },
            upload_bytes_per_second: if seconds > 0.0 {
                delta.tx_bytes as f64 / seconds
            } else {
                0.0
            },
            session: self.session.into(),
            lifetime: lifetime.into(),
        };
        self.previous = Some(raw);
        self.previous_at = Some(now);
        let checkpoint = now.duration_since(self.last_checkpoint) >= Duration::from_secs(10);
        if checkpoint {
            self.last_checkpoint = now;
        }
        (self.latest.clone(), checkpoint)
    }

    fn persisted(&self) -> (String, crate::settings::NetworkTotals) {
        (
            self.profile_id.clone(),
            self.baseline.add(self.session).into(),
        )
    }
}

/// A question the helper is blocked on. Closing the window destroys it, so the
/// prompt has to outlive the webview that first showed it: without this, a
/// window reopened mid-attempt would leave the helper waiting forever for an
/// answer nobody is being asked for any more.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PendingPrompt {
    Certificate {
        fingerprint: String,
        details: String,
    },
    Mfa {
        message: String,
    },
    Gateways {
        list: Vec<Gateway>,
    },
}

#[derive(Clone, Default)]
pub struct VpnRuntime {
    state: Arc<Mutex<String>>,
    last_fingerprint: Arc<Mutex<Option<String>>>,
    profile: Arc<Mutex<Option<ActiveProfile>>>,
    connection: Arc<Mutex<Option<ConnectionInfo>>>,
    pending: Arc<Mutex<Option<PendingPrompt>>>,
    connect_request: Arc<Mutex<Option<String>>>,
    stats: Arc<Mutex<Option<StatsTracker>>>,
}

impl VpnRuntime {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new("disconnected".into())),
            last_fingerprint: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            connection: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(None)),
            connect_request: Arc::new(Mutex::new(None)),
            stats: Arc::new(Mutex::new(None)),
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

    pub fn stats(&self) -> Option<NetworkStats> {
        self.stats
            .lock()
            .ok()?
            .as_ref()
            .map(|stats| stats.latest.clone())
    }

    pub fn start_stats(&self, profile_id: String, baseline: crate::settings::NetworkTotals) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = Some(StatsTracker::new(profile_id, baseline));
        }
    }

    fn record_stats(&self, raw: RawCounters) -> Option<(NetworkStats, bool)> {
        self.stats
            .lock()
            .ok()?
            .as_mut()
            .map(|stats| stats.record(raw))
    }

    fn persisted_stats(&self) -> Option<(String, crate::settings::NetworkTotals)> {
        self.stats
            .lock()
            .ok()?
            .as_ref()
            .map(StatsTracker::persisted)
    }

    pub fn clear_stats(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = None;
        }
    }

    pub fn pending(&self) -> Option<PendingPrompt> {
        self.pending.lock().ok()?.clone()
    }

    /// The connection the tray was told to start. It waits here rather than
    /// being started outright: only a window can ask for the password or the
    /// one-time passcode the attempt may need, and the tray's window may not
    /// exist yet when the request is made.
    pub fn connect_request(&self) -> Option<String> {
        self.connect_request.lock().ok()?.clone()
    }

    pub fn set_connect_request(&self, profile_id: Option<String>) {
        if let Ok(mut current) = self.connect_request.lock() {
            *current = profile_id;
        }
    }

    pub fn set_profile(&self, profile: Option<ActiveProfile>) {
        if let Ok(mut current) = self.profile.lock() {
            *current = profile;
        }
    }

    pub fn set_pending(&self, prompt: Option<PendingPrompt>) {
        if let Ok(mut current) = self.pending.lock() {
            *current = prompt;
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
            self.set_pending(None);
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
        crate::tray::refresh(app, &state, profile.as_ref());
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
                    HelperEvent::State { state } => {
                        if state == "disconnected" {
                            persist_stats(&app, &runtime);
                        }
                        runtime.publish_state(&app, state);
                    }
                    HelperEvent::Log { level, msg } => eprintln!("[gp-helper/{level}] {msg}"),
                    HelperEvent::CertUntrusted {
                        fingerprint,
                        details,
                    } => {
                        if let Ok(mut last) = runtime.last_fingerprint.lock() {
                            *last = Some(fingerprint.clone());
                        }
                        runtime.set_pending(Some(PendingPrompt::Certificate {
                            fingerprint: fingerprint.clone(),
                            details: details.clone(),
                        }));
                        let _ = app.emit(
                            "vpn://cert-untrusted",
                            serde_json::json!({ "fingerprint": fingerprint, "details": details }),
                        );
                    }
                    HelperEvent::MfaChallenge { message } => {
                        runtime.set_pending(Some(PendingPrompt::Mfa {
                            message: message.clone(),
                        }));
                        let _ = app.emit(
                            "vpn://mfa-challenge",
                            serde_json::json!({ "message": message }),
                        );
                    }
                    HelperEvent::Gateways { list, selecting } => {
                        // Only a selecting list is a question; the other kind
                        // is the helper announcing what it found.
                        if selecting {
                            runtime
                                .set_pending(Some(PendingPrompt::Gateways { list: list.clone() }));
                        }
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
                    HelperEvent::Stats {
                        rx_bytes,
                        tx_bytes,
                        rx_packets,
                        tx_packets,
                    } => {
                        let raw = RawCounters {
                            rx_bytes,
                            tx_bytes,
                            rx_packets,
                            tx_packets,
                        };
                        if let Some((stats, checkpoint)) = runtime.record_stats(raw) {
                            let _ = app.emit("vpn://stats", &stats);
                            if checkpoint {
                                persist_stats(&app, &runtime);
                            }
                        }
                    }
                    HelperEvent::Error { msg } => {
                        // The attempt is over: a question still waiting would
                        // only collect an answer nothing is listening for.
                        runtime.set_pending(None);
                        let _ = app.emit("vpn://error", serde_json::json!({ "msg": msg }));
                    }
                }
            }
            persist_stats(&app, &runtime);
            runtime.publish_state(&app, "disconnected".into());
            runtime.set_profile(None);
            runtime.clear_stats();
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

fn persist_stats(app: &AppHandle, runtime: &VpnRuntime) {
    let Some((profile_id, totals)) = runtime.persisted_stats() else {
        return;
    };
    if let Err(error) =
        tauri::async_runtime::block_on(crate::settings::set_totals(app, &profile_id, totals))
    {
        eprintln!("[gp-client] could not save network statistics: {error}");
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
    pub stats: Option<NetworkStats>,
    /// Whatever the helper is still waiting on, so a window opened after the
    /// prompt was raised can put the question back on screen.
    pub pending: Option<PendingPrompt>,
    /// A connection the tray asked for, waiting for a window to run it. A
    /// window built by that very request finds it here, since the event
    /// announcing it was emitted before the webview could listen.
    pub requested_profile_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_snapshot_counts_toward_the_session() {
        let mut tracker = StatsTracker::new(
            "profile".into(),
            crate::settings::NetworkTotals {
                rx_bytes: 100,
                tx_bytes: 200,
                rx_packets: 3,
                tx_packets: 4,
            },
        );
        tracker.record(RawCounters {
            rx_bytes: 25,
            tx_bytes: 50,
            rx_packets: 1,
            tx_packets: 2,
        });

        assert_eq!(tracker.session.rx_bytes, 25);
        assert_eq!(tracker.session.tx_bytes, 50);
        assert_eq!(tracker.persisted().1.rx_bytes, 125);
        assert_eq!(tracker.persisted().1.tx_packets, 6);
    }

    #[test]
    fn cumulative_snapshots_add_only_the_difference() {
        let mut tracker = StatsTracker::new("profile".into(), Default::default());
        tracker.record(RawCounters {
            rx_bytes: 100,
            tx_bytes: 80,
            rx_packets: 10,
            tx_packets: 8,
        });
        tracker.record(RawCounters {
            rx_bytes: 140,
            tx_bytes: 100,
            rx_packets: 14,
            tx_packets: 10,
        });

        assert_eq!(tracker.session.rx_bytes, 140);
        assert_eq!(tracker.session.tx_bytes, 100);
        assert_eq!(tracker.session.rx_packets, 14);
    }

    #[test]
    fn a_reset_native_counter_never_reduces_totals() {
        let before = RawCounters {
            rx_bytes: 500,
            tx_bytes: 300,
            rx_packets: 50,
            tx_packets: 30,
        };
        let reset = RawCounters {
            rx_bytes: 20,
            tx_bytes: 10,
            rx_packets: 2,
            tx_packets: 1,
        };

        assert_eq!(reset.delta(Some(before)).rx_bytes, 20);
        assert_eq!(reset.delta(Some(before)).tx_packets, 1);
    }

    #[test]
    fn adding_counters_saturates_instead_of_wrapping() {
        let maximum = RawCounters {
            rx_bytes: u64::MAX,
            ..Default::default()
        };
        assert_eq!(
            maximum
                .add(RawCounters {
                    rx_bytes: 1,
                    ..Default::default()
                })
                .rx_bytes,
            u64::MAX
        );
    }
}
