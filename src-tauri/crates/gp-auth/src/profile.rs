//! The client identity GlobalProtect servers check before they let us in.
//!
//! openconnect's built-in GlobalProtect support sends `computer=localhost`,
//! `os-version=linux-64` and no `host-id`/`serialno`/`clientgpversion` at all,
//! which PAN-OS deployments that enforce a client version or device identity
//! reject with HTTP 512. Everything here exists to send what a real
//! GlobalProtect client sends.

use std::{ffi::CStr, fs};

use sha2::{Digest, Sha256};

const CLIENT_VERSION_LINUX: &str = "6.3.3-619";
const CLIENT_VERSION_WINDOWS: &str = "6.3.3-650";
const CLIENT_VERSION_MACOS: &str = "6.3.3-915";

const DEFAULT_LINUX_DISTRO: &str = "Ubuntu 24.04.3 LTS";
const DEFAULT_WINDOWS_DISTRO: &str = "Windows 11 Pro";
const DEFAULT_MACOS_VERSION: &str = "13.4.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientOs {
	#[default]
	Linux,
	Windows,
	Mac,
}

impl ClientOs {
	/// Parses the value stored in settings; anything unrecognised means Linux.
	pub fn parse(value: &str) -> Self {
		match value.trim().to_ascii_lowercase().as_str() {
			"windows" | "win" => ClientOs::Windows,
			"mac" | "macos" | "mac-intel" => ClientOs::Mac,
			_ => ClientOs::Linux,
		}
	}

	/// The `clientos` form value.
	pub fn as_str(self) -> &'static str {
		match self {
			ClientOs::Linux => "Linux",
			ClientOs::Windows => "Windows",
			ClientOs::Mac => "Mac",
		}
	}

	/// The matching `openconnect_set_reported_os()` value; the library rejects
	/// anything outside its own allow-list.
	pub fn openconnect_os(self) -> &'static str {
		match self {
			ClientOs::Linux => "linux-64",
			ClientOs::Windows => "win",
			ClientOs::Mac => "mac-intel",
		}
	}

	fn client_version(self) -> &'static str {
		match self {
			ClientOs::Linux => CLIENT_VERSION_LINUX,
			ClientOs::Windows => CLIENT_VERSION_WINDOWS,
			ClientOs::Mac => CLIENT_VERSION_MACOS,
		}
	}

	/// Placeholder password a SAML/cookie login sends in the `passwd` field.
	pub fn saml_password(self) -> &'static str {
		match self {
			ClientOs::Linux => "SAMLPASS",
			ClientOs::Windows | ClientOs::Mac => "",
		}
	}

	/// Whether `csc-support=yes` is advertised on the portal config request.
	pub fn csc_support(self) -> bool {
		!matches!(self, ClientOs::Linux)
	}

	/// Windows clients send the prelogin parameters as a query string; the
	/// others send them as a form body.
	pub fn prelogin_params_in_query(self) -> bool {
		matches!(self, ClientOs::Windows)
	}

	pub fn kerberos_support_in_query(self) -> bool {
		matches!(self, ClientOs::Windows | ClientOs::Mac)
	}

	fn os_version(self) -> String {
		match self {
			ClientOs::Linux => format!("Linux {}", linux_distro()),
			ClientOs::Windows => format!("Microsoft {DEFAULT_WINDOWS_DISTRO} , 64-bit"),
			ClientOs::Mac => format!("Apple Mac OS X {DEFAULT_MACOS_VERSION}"),
		}
	}
}

/// Everything a GlobalProtect request needs to describe "this machine".
#[derive(Debug, Clone)]
pub struct Profile {
	pub client_os: ClientOs,
	pub os_version: String,
	pub client_version: String,
	pub computer: String,
	pub host_id: String,
	pub serialno: String,
	pub user_agent: String,
}

impl Profile {
	/// Builds a profile from the running machine, reporting `client_os`.
	///
	/// Spoofing a different OS only changes the advertised strings; the machine
	/// identifiers stay real so the server sees a stable device.
	pub fn detect(client_os: ClientOs) -> Self {
		let os_version = client_os.os_version();
		let client_version = client_os.client_version().to_owned();
		let host_id = host_id();
		Self {
			user_agent: format!("PAN GlobalProtect/{client_version} ({os_version})"),
			serialno: serialno(&host_id),
			host_id,
			computer: hostname(),
			os_version,
			client_version,
			client_os,
		}
	}
}

fn linux_distro() -> String {
	let Ok(contents) = fs::read_to_string("/etc/os-release") else {
		return DEFAULT_LINUX_DISTRO.to_owned();
	};
	contents
		.lines()
		.find_map(|line| line.strip_prefix("PRETTY_NAME="))
		.map(|value| value.trim_matches('"').to_owned())
		.filter(|value| !value.is_empty())
		.unwrap_or_else(|| DEFAULT_LINUX_DISTRO.to_owned())
}

fn hostname() -> String {
	let mut buffer = [0 as libc::c_char; 256];
	// SAFETY: the buffer outlives the call and is one byte longer than the
	// length handed to gethostname, so the result is always NUL terminated.
	let result = unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len() - 1) };
	if result == 0 {
		let name = unsafe { CStr::from_ptr(buffer.as_ptr()) }
			.to_string_lossy()
			.into_owned();
		if !name.is_empty() {
			return name;
		}
	}
	fs::read_to_string("/etc/hostname")
		.map(|value| value.trim().to_owned())
		.ok()
		.filter(|value| !value.is_empty())
		.unwrap_or_else(|| "localhost".to_owned())
}

/// The machine UUID a GlobalProtect client reports as `host-id`.
///
/// `product_uuid` is only readable by root, which the privileged helper is; the
/// other sources keep the value stable for unprivileged callers such as the
/// `probe` example.
fn host_id() -> String {
	if let Some(uuid) = read_trimmed("/sys/class/dmi/id/product_uuid").and_then(|v| normalize_uuid(&v)) {
		return uuid;
	}
	if let Some(machine_id) = read_trimmed("/etc/machine-id").and_then(|v| uuid_from_hex(&v)) {
		return machine_id;
	}
	derive_uuid(&format!("gp-auth-host-id:{}", hostname()))
}

fn serialno(host_id: &str) -> String {
	read_trimmed("/sys/class/dmi/id/product_serial")
		.filter(|value| !value.is_empty() && value != "None")
		.or_else(|| vmware_serial_from_uuid(host_id))
		.unwrap_or_else(|| host_id.to_owned())
}

fn read_trimmed(path: &str) -> Option<String> {
	fs::read_to_string(path)
		.ok()
		.map(|value| value.trim().to_owned())
		.filter(|value| !value.is_empty())
}

fn normalize_uuid(value: &str) -> Option<String> {
	let value = value.trim().to_ascii_lowercase();
	let groups: Vec<&str> = value.split('-').collect();
	let expected = [8, 4, 4, 4, 12];
	if groups.len() != expected.len() {
		return None;
	}
	for (group, length) in groups.iter().zip(expected) {
		if group.len() != length || !group.chars().all(|ch| ch.is_ascii_hexdigit()) {
			return None;
		}
	}
	Some(value)
}

fn uuid_from_hex(value: &str) -> Option<String> {
	let value = value.trim().to_ascii_lowercase();
	if value.len() != 32 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
		return None;
	}
	Some(format!(
		"{}-{}-{}-{}-{}",
		&value[0..8],
		&value[8..12],
		&value[12..16],
		&value[16..20],
		&value[20..32]
	))
}

/// Deterministic UUID-shaped identifier for machines without usable DMI data.
fn derive_uuid(seed: &str) -> String {
	let digest = Sha256::digest(seed.as_bytes());
	let hex: String = digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect();
	uuid_from_hex(&hex).unwrap_or_else(|| hex.clone())
}

/// Reproduces the SMBIOS byte order a VMware guest reports as its serial, which
/// is the shape GlobalProtect clients send when no DMI serial is readable.
fn vmware_serial_from_uuid(uuid: &str) -> Option<String> {
	let normalized = normalize_uuid(uuid)?;
	let groups: Vec<&str> = normalized.split('-').collect();
	let compact = format!(
		"{}{}{}{}{}",
		reverse_hex_bytes(groups[0]),
		reverse_hex_bytes(groups[1]),
		reverse_hex_bytes(groups[2]),
		groups[3],
		groups[4]
	);
	let pairs: Vec<String> = (0..16).map(|index| compact[index * 2..index * 2 + 2].to_owned()).collect();
	Some(format!(
		"VMware-{}-{}",
		pairs[0..8].join(" "),
		pairs[8..16].join(" ")
	))
}

fn reverse_hex_bytes(value: &str) -> String {
	value
		.as_bytes()
		.chunks(2)
		.rev()
		.filter_map(|chunk| std::str::from_utf8(chunk).ok())
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn client_os_parsing_defaults_to_linux() {
		assert_eq!(ClientOs::parse("Windows"), ClientOs::Windows);
		assert_eq!(ClientOs::parse("mac-intel"), ClientOs::Mac);
		assert_eq!(ClientOs::parse("nonsense"), ClientOs::Linux);
	}

	#[test]
	fn reported_os_matches_openconnect_allow_list() {
		assert_eq!(ClientOs::Linux.openconnect_os(), "linux-64");
		assert_eq!(ClientOs::Windows.openconnect_os(), "win");
		assert_eq!(ClientOs::Mac.openconnect_os(), "mac-intel");
	}

	#[test]
	fn vmware_serial_uses_smbios_byte_order() {
		assert_eq!(
			vmware_serial_from_uuid("5a784d56-6461-19ac-9ea9-d36a3b9c6cef").unwrap(),
			"VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef"
		);
	}

	#[test]
	fn invalid_uuid_has_no_vmware_serial() {
		assert_eq!(vmware_serial_from_uuid("not-a-uuid"), None);
	}

	#[test]
	fn machine_id_becomes_a_uuid() {
		assert_eq!(
			uuid_from_hex("0123456789abcdef0123456789abcdef").unwrap(),
			"01234567-89ab-cdef-0123-456789abcdef"
		);
	}

	#[test]
	fn user_agent_identifies_as_a_globalprotect_client() {
		let profile = Profile::detect(ClientOs::Windows);
		assert!(profile.user_agent.starts_with("PAN GlobalProtect/6.3.3-650 ("));
		assert_eq!(profile.os_version, "Microsoft Windows 11 Pro , 64-bit");
	}
}
