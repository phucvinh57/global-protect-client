//! Saved connection profiles. Passwords live in the keyring, never in this file.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub id: String,
    /// Label shown in the connection list.
    #[serde(default)]
    pub name: String,
    pub portal: String,
    pub username: String,
    /// Whether the password for this profile belongs in the keyring.
    #[serde(default)]
    pub remember: bool,
    #[serde(default)]
    pub cafile: Option<String>,
    #[serde(default)]
    pub client_cert: Option<String>,
    #[serde(default)]
    pub client_key: Option<String>,
    #[serde(default)]
    pub trusted_fingerprint: Option<String>,
    /// Gateway to log into directly, skipping portal discovery.
    #[serde(default)]
    pub gateway: Option<String>,
    /// Client OS reported to the portal: `Linux`, `Windows` or `Mac`.
    #[serde(default)]
    pub client_os: Option<String>,
    /// Override for vpnc-script discovery.
    #[serde(default)]
    pub script: Option<String>,
    /// Submit a HIP report when the gateway asks for one.
    #[serde(default)]
    pub hip: bool,
    /// Ask for a one-time passcode before every attempt, even when the password
    /// comes from the keyring. Portals that want the code appended to the
    /// password never issue a challenge, so nothing else can prompt for it.
    #[serde(default)]
    pub requires_otp: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

impl Store {
    pub fn find(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }
}

/// Monotonic enough for local identifiers; profiles are only ever created by
/// this process, one user interaction at a time.
pub fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("p{nanos:x}")
}

/// Falls back to the portal host so a profile always has something to show.
pub fn default_name(portal: &str) -> String {
    let trimmed = portal
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let host = trimmed.split('/').next().unwrap_or(trimmed);
    if host.is_empty() {
        "New connection".into()
    } else {
        host.to_string()
    }
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Result<Store, String> {
    let contents = match fs::read_to_string(path(app)?) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => return Err(error.to_string()),
    };
    let value: Value = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    if value.get("profiles").is_some() {
        return serde_json::from_value(value).map_err(|error| error.to_string());
    }
    // Files written before profiles existed hold a single connection inline.
    let mut profile: Profile = serde_json::from_value(value).map_err(|error| error.to_string())?;
    profile.id = new_id();
    profile.name = default_name(&profile.portal);
    let store = Store {
        profiles: vec![profile],
    };
    // Written back straight away, otherwise every load would mint a new id and
    // no connection could be referred to twice.
    save(app, &store)?;
    Ok(store)
}

pub fn save(app: &AppHandle, store: &Store) -> Result<(), String> {
    let path = path(app)?;
    let contents = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| error.to_string())
}
