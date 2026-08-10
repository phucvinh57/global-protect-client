//! System tray. Closing the window destroys it, so the tray icon is the app's
//! only visible surface while a tunnel is up, and the only way back to a
//! window.

use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewWindowBuilder,
};

use crate::{helper_process::ActiveProfile, settings};

pub const TRAY_ID: &str = "gp-tray";
/// Prefix on the id of every saved-connection item, so one handler recognises
/// them whatever the profile is called.
const CONNECT_PREFIX: &str = "connect:";

const CONNECTED_ICON: &[u8] = include_bytes!("../icons/tray-connected.png");
const DISCONNECTED_ICON: &[u8] = include_bytes!("../icons/tray-disconnected.png");

fn icon(connected: bool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if connected {
        CONNECTED_ICON
    } else {
        DISCONNECTED_ICON
    })
}

/// Raises the window, building a new one when the last close destroyed it.
/// The rebuild reuses the config the app first started from, so a reopened
/// window is indistinguishable from the original.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let Some(config) = app.config().app.windows.first().cloned() else {
        return;
    };
    let built = WebviewWindowBuilder::from_config(app, &config).and_then(|builder| builder.build());
    if let Err(error) = built {
        eprintln!("[gp-client] could not reopen the window: {error}");
    }
}

/// An `&` in a menu label marks the next character as a mnemonic, so a
/// connection named with one has to say it twice to be drawn.
fn label(name: &str) -> String {
    name.replace('&', "&&")
}

/// The menu is rebuilt on every state change so its labels can describe the
/// current connection instead of being generic, and so the saved connections
/// it lists stay in step with the ones the window has saved.
fn menu(
    app: &AppHandle,
    state: &str,
    profile: Option<&ActiveProfile>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let name = profile.map(|profile| profile.name.as_str());
    let status = match (state, name) {
        ("connected", Some(name)) => format!("Connected to {name}"),
        ("connected", None) => "Connected".into(),
        ("authenticating", Some(name)) => format!("Authenticating to {name}…"),
        ("connecting", Some(name)) => format!("Connecting to {name}…"),
        ("authenticating" | "connecting", None) => "Connecting…".into(),
        ("disconnecting", _) => "Disconnecting…".into(),
        _ => "Not connected".into(),
    };
    let idle = state == "disconnected";

    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "status",
        status,
        false,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    // Every saved connection, so a tunnel can be started from the tray alone.
    // Only one tunnel exists at a time, so none of them can be picked while
    // another is up; the one that is up is the one that is ticked.
    let profiles = settings::load(app)
        .map(|store| store.profiles)
        .unwrap_or_default();
    if profiles.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "no-connections",
            "No saved connections",
            false,
            None::<&str>,
        )?)?;
    }
    for saved in &profiles {
        menu.append(&CheckMenuItem::with_id(
            app,
            format!("{CONNECT_PREFIX}{}", saved.id),
            label(&saved.name),
            idle,
            profile.is_some_and(|active| active.id == saved.id),
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    menu.append(&MenuItem::with_id(
        app,
        "show",
        "Open gp-client",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "disconnect",
        "Disconnect",
        !idle && state != "disconnecting",
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

pub fn build(
    app: &AppHandle,
    on_connect: fn(&AppHandle, &str),
    on_disconnect: fn(&AppHandle),
    on_quit: fn(&AppHandle),
) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false)?)
        .icon_as_template(false)
        .tooltip("gp-client — not connected")
        .menu(&menu(app, "disconnected", None)?)
        // Left click opens the menu. Linux's appindicator delivers no click
        // events at all, so "Open gp-client" in the menu has to be the way
        // back to the window on every platform.
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            if let Some(profile_id) = id.strip_prefix(CONNECT_PREFIX) {
                on_connect(app, profile_id);
                return;
            }
            match id {
                "show" => show_window(app),
                "disconnect" => on_disconnect(app),
                "quit" => on_quit(app),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Repaints the tray for a new VPN state, or for a change to the saved
/// connections it lists. Failures are ignored: a stale tray icon must never
/// break a connection.
pub fn refresh(app: &AppHandle, state: &str, profile: Option<&ActiveProfile>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(image) = icon(state == "connected") {
        let _ = tray.set_icon(Some(image));
    }
    let tooltip = match (state, profile.map(|profile| profile.name.as_str())) {
        ("connected", Some(name)) => format!("gp-client — connected to {name}"),
        ("connected", None) => "gp-client — connected".into(),
        ("disconnected", _) => "gp-client — not connected".into(),
        (other, Some(name)) => format!("gp-client — {other} ({name})"),
        (other, None) => format!("gp-client — {other}"),
    };
    let _ = tray.set_tooltip(Some(tooltip));
    if let Ok(menu) = menu(app, state, profile) {
        let _ = tray.set_menu(Some(menu));
    }
}
