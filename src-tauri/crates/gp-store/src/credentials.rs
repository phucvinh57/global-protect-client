//! Secret Service / native keyring wrapper. Settings never contain passwords.

use std::sync::OnceLock;

const SERVICE: &str = "com.npvinh.gp-client";

fn account(portal: &str, username: &str) -> String {
    format!("{portal}\n{username}")
}

fn entry(portal: &str, username: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &account(portal, username)).map_err(|error| error.to_string())
}

/// The keyring crate can be compiled without libsecret. A missing Secret Service
/// is a normal desktop configuration, so callers should keep the form usable.
pub fn is_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let Ok(entry) = entry("availability-probe", "gp-client") else {
            return false;
        };
        match entry.get_password() {
            Ok(_) => true,
            Err(keyring::Error::NoEntry) => true,
            Err(keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_)) => false,
            Err(_) => true,
        }
    })
}

pub fn get(portal: &str, username: &str) -> Option<String> {
    entry(portal, username).ok()?.get_password().ok()
}

pub fn set(portal: &str, username: &str, password: &str) -> Result<(), String> {
    entry(portal, username)?
        .set_password(password)
        .map_err(|error| error.to_string())
}

pub fn delete(portal: &str, username: &str) -> Result<(), String> {
    match entry(portal, username)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
