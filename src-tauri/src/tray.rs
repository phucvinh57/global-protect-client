//! System tray. Closing the window only hides it, so the tray icon is the
//! app's only visible surface while a tunnel is up.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub const TRAY_ID: &str = "gp-tray";

const CONNECTED_ICON: &[u8] = include_bytes!("../icons/tray-connected.png");
const DISCONNECTED_ICON: &[u8] = include_bytes!("../icons/tray-disconnected.png");

fn icon(connected: bool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if connected {
        CONNECTED_ICON
    } else {
        DISCONNECTED_ICON
    })
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// The menu is rebuilt on every state change so its labels can describe the
/// current connection instead of being generic.
fn menu(app: &AppHandle, state: &str, profile: Option<&str>) -> tauri::Result<Menu<tauri::Wry>> {
    let status = match (state, profile) {
        ("connected", Some(name)) => format!("Connected to {name}"),
        ("connected", None) => "Connected".into(),
        ("authenticating", Some(name)) => format!("Authenticating to {name}…"),
        ("connecting", Some(name)) => format!("Connecting to {name}…"),
        ("authenticating" | "connecting", None) => "Connecting…".into(),
        ("disconnecting", _) => "Disconnecting…".into(),
        _ => "Not connected".into(),
    };
    let status_item = MenuItem::with_id(app, "status", status, false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Open gp-client", true, None::<&str>)?;
    let disconnect = MenuItem::with_id(
        app,
        "disconnect",
        "Disconnect",
        state != "disconnected" && state != "disconnecting",
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &status_item,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &disconnect,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

pub fn build(
    app: &AppHandle,
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
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => show_window(app),
            "disconnect" => on_disconnect(app),
            "quit" => on_quit(app),
            _ => {}
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

/// Repaints the tray for a new VPN state. Failures are ignored: a stale tray
/// icon must never break a connection.
pub fn refresh(app: &AppHandle, state: &str, profile: Option<&str>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(image) = icon(state == "connected") {
        let _ = tray.set_icon(Some(image));
    }
    let tooltip = match (state, profile) {
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
