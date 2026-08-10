mod credentials;
mod helper_process;
mod settings;
mod tray;

use std::{sync::Mutex, thread, time::Duration};

use helper_process::{ActiveProfile, HelperProcess, StatusResponse, VpnRuntime};
use serde::{Deserialize, Serialize};
use settings::Profile;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

struct AppState {
    helper: Mutex<Option<HelperProcess>>,
    runtime: VpnRuntime,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectInput {
    profile_id: String,
    /// Supplied by the credentials prompt when nothing is in the keyring.
    password: Option<String>,
    otp: Option<String>,
    /// Set when the prompt's "remember me" differs from the stored profile.
    remember: Option<bool>,
}

/// A profile as the window sees it: never the password itself, only whether
/// one is waiting in the keyring.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    #[serde(flatten)]
    profile: Profile,
    has_saved_password: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfilesResponse {
    profiles: Vec<ProfileView>,
    credentials_available: bool,
}

fn view(profile: Profile) -> ProfileView {
    let has_saved_password = profile.remember
        && credentials::is_available()
        && credentials::get(&profile.portal, &profile.username).is_some();
    ProfileView {
        profile,
        has_saved_password,
    }
}

/// Answers the question the helper is blocked on, failing when there is no
/// helper. The prompt is forgotten once the answer is on its way, so a window
/// opened later is not asked the same question a second time.
fn answer_prompt(state: &State<'_, AppState>, command: serde_json::Value) -> Result<(), String> {
    let mut helper = state
        .helper
        .lock()
        .map_err(|_| "VPN helper state is unavailable")?;
    helper
        .as_mut()
        .ok_or("VPN helper is not running")?
        .send(command)?;
    state.runtime.set_pending(None);
    Ok(())
}

/// Repaints the tray from the runtime as it stands. The tray menu lists the
/// saved connections, so it has to be rebuilt whenever that list changes and
/// not only when the tunnel does.
fn refresh_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let profile = state.runtime.profile();
    tray::refresh(app, &state.runtime.state(), profile.as_ref());
}

#[tauri::command]
fn profiles_load(app: AppHandle) -> Result<ProfilesResponse, String> {
    let store = settings::load(&app)?;
    Ok(ProfilesResponse {
        profiles: store.profiles.into_iter().map(view).collect(),
        credentials_available: credentials::is_available(),
    })
}

/// Creates or updates one profile. `password` is only read when the profile
/// asks to be remembered; it is written to the keyring and never to disk.
#[tauri::command]
fn profile_save(
    app: AppHandle,
    profile: Profile,
    password: Option<String>,
) -> Result<ProfileView, String> {
    let mut profile = profile;
    if profile.portal.trim().is_empty() || profile.username.trim().is_empty() {
        return Err("Portal and username are required".into());
    }
    if profile.name.trim().is_empty() {
        profile.name = settings::default_name(&profile.portal);
    }
    let mut store = settings::load(&app)?;
    let previous = store.find(&profile.id).cloned();
    match store
        .profiles
        .iter_mut()
        .find(|existing| existing.id == profile.id)
    {
        Some(existing) => *existing = profile.clone(),
        None => {
            profile.id = settings::new_id();
            store.profiles.push(profile.clone());
        }
    }
    settings::save(&app, &store)?;

    // The keyring is addressed by portal and username, so an edit to either
    // would otherwise leave the old secret behind.
    if let Some(previous) = previous.filter(|previous| {
        previous.portal != profile.portal || previous.username != profile.username
    }) {
        let _ = credentials::delete(&previous.portal, &previous.username);
    }
    if profile.remember && credentials::is_available() {
        if let Some(password) = password.filter(|password| !password.is_empty()) {
            credentials::set(&profile.portal, &profile.username, &password)?;
        }
    } else if !profile.remember {
        let _ = credentials::delete(&profile.portal, &profile.username);
    }
    refresh_tray(&app);
    Ok(view(profile))
}

#[tauri::command]
fn profile_delete(app: AppHandle, id: String) -> Result<(), String> {
    let mut store = settings::load(&app)?;
    if let Some(profile) = store.find(&id).cloned() {
        let _ = credentials::delete(&profile.portal, &profile.username);
    }
    store.profiles.retain(|profile| profile.id != id);
    settings::save(&app, &store)?;
    refresh_tray(&app);
    Ok(())
}

#[tauri::command]
fn vpn_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ConnectInput,
) -> Result<(), String> {
    if state.runtime.state() != "disconnected" {
        return Err("Another connection is already active".into());
    }
    let mut store = settings::load(&app)?;
    let mut profile = store
        .find(&input.profile_id)
        .cloned()
        .ok_or("That connection no longer exists")?;

    // A prompt may have changed the "remember me" choice for this attempt.
    if let Some(remember) = input.remember {
        if remember != profile.remember {
            profile.remember = remember;
            if let Some(stored) = store
                .profiles
                .iter_mut()
                .find(|stored| stored.id == profile.id)
            {
                stored.remember = remember;
            }
            settings::save(&app, &store)?;
        }
    }

    let password = match input.password.filter(|password| !password.is_empty()) {
        Some(password) => {
            if profile.remember && credentials::is_available() {
                credentials::set(&profile.portal, &profile.username, &password)?;
            }
            password
        }
        None => credentials::get(&profile.portal, &profile.username)
            .ok_or("This connection has no saved password")?,
    };
    if !profile.remember {
        let _ = credentials::delete(&profile.portal, &profile.username);
    }

    let mut helper = state
        .helper
        .lock()
        .map_err(|_| "VPN helper state is unavailable")?;
    if helper.as_mut().is_some_and(HelperProcess::is_finished) {
        *helper = None;
    }
    state.runtime.set_profile(Some(ActiveProfile {
        id: profile.id.clone(),
        name: profile.name.clone(),
    }));
    if helper.is_none() {
        match HelperProcess::spawn(app.clone(), state.runtime.clone()) {
            Ok(spawned) => *helper = Some(spawned),
            Err(error) => {
                state.runtime.set_profile(None);
                return Err(error);
            }
        }
    }
    let result = helper
        .as_mut()
        .ok_or("VPN helper did not start")?
        .send(serde_json::json!({
            "cmd": "connect",
            "portal": profile.portal,
            "username": profile.username,
            "password": password,
            "otp": input.otp.unwrap_or_default(),
            "gateway": profile.gateway,
            "client_os": profile.client_os,
            "cafile": profile.cafile,
            "client_cert": profile.client_cert,
            "client_key": profile.client_key,
            "trusted_fingerprint": profile.trusted_fingerprint,
            "script": profile.script,
            "hip": profile.hip,
            "verbose": false,
        }));
    if result.is_err() {
        state.runtime.set_profile(None);
    }
    result
}

#[tauri::command]
fn vpn_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let mut helper = state
        .helper
        .lock()
        .map_err(|_| "VPN helper state is unavailable")?;
    if let Some(helper) = helper.as_mut() {
        helper.send(serde_json::json!({ "cmd": "disconnect" }))?;
    }
    Ok(())
}

#[tauri::command]
fn vpn_status(state: State<'_, AppState>) -> StatusResponse {
    StatusResponse {
        state: state.runtime.state(),
        profile_id: state.runtime.profile().map(|profile| profile.id),
        connection: state.runtime.connection(),
        pending: state.runtime.pending(),
        requested_profile_id: state.runtime.connect_request(),
    }
}

/// Forgets the tray's connection request once a window has taken it on, so a
/// window opened later never starts the same attempt a second time. The tray
/// is repainted with it: clicking one of its connections ticks that item, and
/// only a tunnel that is actually up should stay ticked.
#[tauri::command]
fn vpn_clear_connect_request(app: AppHandle, state: State<'_, AppState>) {
    state.runtime.set_connect_request(None);
    refresh_tray(&app);
}

#[tauri::command]
fn vpn_trust_cert(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let fingerprint = state
        .runtime
        .last_fingerprint()
        .ok_or("There is no certificate awaiting a trust decision")?;
    let profile_id = state
        .runtime
        .profile()
        .map(|profile| profile.id)
        .ok_or("There is no connection in progress")?;
    let mut store = settings::load(&app)?;
    if let Some(profile) = store
        .profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
    {
        profile.trusted_fingerprint = Some(fingerprint);
        settings::save(&app, &store)?;
    }
    answer_prompt(&state, serde_json::json!({ "cmd": "trust_cert" }))
}

#[tauri::command]
fn vpn_reject_cert(state: State<'_, AppState>) -> Result<(), String> {
    answer_prompt(&state, serde_json::json!({ "cmd": "reject_cert" }))
}

/// Answers a gateway multi-factor challenge.
#[tauri::command]
fn vpn_submit_otp(state: State<'_, AppState>, otp: String) -> Result<(), String> {
    answer_prompt(
        &state,
        serde_json::json!({ "cmd": "submit_otp", "otp": otp }),
    )
}

/// Picks one of the gateways the portal offered.
#[tauri::command]
fn vpn_select_gateway(state: State<'_, AppState>, address: String) -> Result<(), String> {
    answer_prompt(
        &state,
        serde_json::json!({ "cmd": "select_gateway", "address": address }),
    )
}

/// Starts a connection the tray asked for. The attempt is not started here:
/// it may need a password or a one-time passcode, and the window is the only
/// thing that can ask for either, or report the attempt in a toast. So the
/// request is parked in the runtime and a window is opened to run it — a fresh
/// one when the last close destroyed it, which reads the request back from
/// `vpn_status` because this event was emitted before its webview could
/// listen for anything.
fn connect_from_tray(app: &AppHandle, profile_id: &str) {
    let state = app.state::<AppState>();
    if state.runtime.state() != "disconnected" {
        return;
    }
    state
        .runtime
        .set_connect_request(Some(profile_id.to_string()));
    tray::show_window(app);
    let _ = app.emit(
        "vpn://connect-request",
        serde_json::json!({ "profileId": profile_id }),
    );
}

/// Asks the helper to tear the tunnel down. Used by the tray, which has no
/// access to the Tauri command surface.
fn disconnect_from_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut helper) = state.helper.lock() {
        if let Some(helper) = helper.as_mut() {
            let _ = helper.send(serde_json::json!({ "cmd": "disconnect" }));
        }
    };
}

/// Leaving a tunnel up after the app is gone would strand the routing table,
/// so quitting always tries to close it first.
fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.runtime.state() != "disconnected" {
        disconnect_from_tray(app);
        thread::sleep(Duration::from_millis(600));
    }
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            helper: Mutex::new(None),
            runtime: VpnRuntime::new(),
        })
        .setup(|app| {
            tray::build(
                &app.handle().clone(),
                connect_from_tray,
                disconnect_from_tray,
                quit,
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            profiles_load,
            profile_save,
            profile_delete,
            vpn_connect,
            vpn_disconnect,
            vpn_status,
            vpn_clear_connect_request,
            vpn_trust_cert,
            vpn_reject_cert,
            vpn_submit_otp,
            vpn_select_gateway,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // Closing the window destroys it along with its webview process, which
        // is the point: only the tray, the helper and the tunnel stay in
        // memory. The event loop would end with the last window, so that exit
        // is vetoed. Quit's own `AppHandle::exit` carries a code and passes.
        .run(|_app, event| {
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
