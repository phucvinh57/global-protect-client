//! Narrow, panic-safe Rust ownership wrapper around libopenconnect.
//!
//! Authentication is not done here. `gp-auth` speaks the GlobalProtect portal
//! and gateway protocol itself and produces a cookie, which is installed with
//! [`Vpn::configure`]; libopenconnect is only asked to build the tunnel. That
//! split exists because openconnect's own GlobalProtect login cannot send the
//! client identity fields modern PAN-OS deployments insist on, and answers
//! with HTTP 512 when they are missing.

use std::{
	ffi::{CStr, CString},
	os::fd::RawFd,
	panic::{catch_unwind, AssertUnwindSafe},
	path::Path,
	ptr,
	sync::{Arc, Mutex, OnceLock},
};

pub use openconnect_sys as sys;

/// Where distributions put the tunnel-configuration script, most specific
/// first. openconnect cannot bring an interface up without one.
const VPNC_SCRIPT_LOCATIONS: &[&str] = &[
	"/usr/local/libexec/gpclient/vpnc-script",
	"/usr/libexec/gpclient/vpnc-script",
	"/usr/lib/gpclient/vpnc-script",
	"/usr/local/share/vpnc-scripts/vpnc-script",
	"/usr/local/sbin/vpnc-script",
	"/usr/share/vpnc-scripts/vpnc-script",
	"/usr/sbin/vpnc-script",
	"/etc/vpnc/vpnc-script",
	"/etc/openconnect/vpnc-script",
	"/usr/libexec/vpnc-scripts/vpnc-script",
];

/// HIP report generators shipped alongside openconnect. Gateways that demand a
/// host-information report reject or restrict the session without one.
const HIP_SCRIPT_LOCATIONS: &[&str] = &[
	"/usr/local/libexec/gpclient/hipreport.sh",
	"/usr/libexec/gpclient/hipreport.sh",
	"/usr/lib/x86_64-linux-gnu/openconnect/hipreport.sh",
	"/usr/lib/aarch64-linux-gnu/openconnect/hipreport.sh",
	"/usr/lib/openconnect/hipreport.sh",
	"/usr/libexec/openconnect/hipreport.sh",
];

fn first_executable(locations: &[&'static str]) -> Option<&'static str> {
	locations
		.iter()
		.copied()
		.find(|location| is_executable(Path::new(location)))
}

fn is_executable(path: &Path) -> bool {
	use std::os::unix::fs::PermissionsExt;
	std::fs::metadata(path)
		.map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
		.unwrap_or(false)
}

pub fn find_vpnc_script() -> Option<&'static str> {
	first_executable(VPNC_SCRIPT_LOCATIONS)
}

pub fn find_hip_script() -> Option<&'static str> {
	first_executable(HIP_SCRIPT_LOCATIONS)
}

pub trait Callbacks: Send {
	fn progress(&mut self, level: i32, message: &str);
	fn validate_peer_cert(&mut self, vpninfo: *mut sys::openconnect_info, reason: &str) -> i32;
	fn write_new_config(&mut self, _contents: &[u8]) -> i32 {
		0
	}
}

/// Cooperative cancellation usable by a thread other than the VPN worker.
#[derive(Clone)]
pub struct CancelHandle(Arc<Mutex<Option<CancelFd>>>);

enum CancelFd {
	/// The write side is caller-owned until libopenconnect creates its command pipe.
	Auth(RawFd),
	/// Both ends are owned and closed by libopenconnect_vpninfo_free().
	Command(RawFd),
}

impl CancelHandle {
	pub fn cancel(&self) {
		if let Ok(guard) = self.0.lock() {
			if let Some(fd) = guard.as_ref().map(|fd| match fd {
				CancelFd::Auth(fd) | CancelFd::Command(fd) => *fd,
			}) {
				// OC_CMD_CANCEL is deliberately a one-byte command.
				unsafe {
					libc::write(fd, b"x".as_ptr().cast(), 1);
				}
			}
		}
	}
}

struct CallbackContext {
	callbacks: *mut (dyn Callbacks + 'static),
	vpninfo: *mut sys::openconnect_info,
}

pub struct Vpn<'a> {
	info: *mut sys::openconnect_info,
	cancel: CancelHandle,
	cancel_read_fd: RawFd,
	callback_context: Box<CallbackContext>,
	_callback: std::marker::PhantomData<&'a mut dyn Callbacks>,
}

impl<'a> Vpn<'a> {
	pub fn new(callbacks: &'a mut dyn Callbacks, useragent: &str) -> Result<Self, String> {
		static SSL_INIT: OnceLock<i32> = OnceLock::new();
		let init = *SSL_INIT.get_or_init(|| unsafe { sys::openconnect_init_ssl() });
		if init != 0 {
			return Err(format!("openconnect_init_ssl failed: {init}"));
		}

		let mut fds = [-1, -1];
		if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
			return Err(std::io::Error::last_os_error().to_string());
		}
		let useragent = cstring(useragent)?;
		// libopenconnect only retains this pointer while Vpn owns the context.
		let callbacks = unsafe {
			std::mem::transmute::<*mut (dyn Callbacks + 'a), *mut (dyn Callbacks + 'static)>(
				callbacks as *mut dyn Callbacks,
			)
		};
		let mut callback_context = Box::new(CallbackContext {
			callbacks,
			vpninfo: ptr::null_mut(),
		});
		let info = unsafe {
			sys::openconnect_vpninfo_new(
				useragent.as_ptr(),
				Some(validate_peer_cert),
				Some(write_new_config),
				// The cookie makes openconnect's own login unreachable, so the
				// form callback can only ever be a no-op.
				Some(refuse_auth_form),
				Some(sys::gp_progress_trampoline),
				(&mut *callback_context as *mut CallbackContext).cast(),
			)
		};
		if info.is_null() {
			unsafe {
				libc::close(fds[0]);
				libc::close(fds[1]);
			}
			return Err("openconnect_vpninfo_new failed".into());
		}
		unsafe { sys::openconnect_set_cancel_fd(info, fds[0]) };
		callback_context.vpninfo = info;
		Ok(Self {
			info,
			cancel: CancelHandle(Arc::new(Mutex::new(Some(CancelFd::Auth(fds[1]))))),
			cancel_read_fd: fds[0],
			callback_context,
			_callback: std::marker::PhantomData,
		})
	}

	pub fn cancel_handle(&self) -> CancelHandle {
		self.cancel.clone()
	}

	pub fn configure(&mut self, options: &Options) -> Result<(), String> {
		self.call("set protocol", unsafe {
			sys::openconnect_set_protocol(self.info, cstring("gp")?.as_ptr())
		})?;
		self.call("set reported OS", unsafe {
			sys::openconnect_set_reported_os(self.info, cstring(&options.reported_os)?.as_ptr())
		})?;
		unsafe { sys::openconnect_set_loglevel(self.info, options.log_level) };

		if let Some(path) = non_empty(options.cafile.as_deref()) {
			let path = cstring(path)?;
			self.call("set CA file", unsafe { sys::openconnect_set_cafile(self.info, path.as_ptr()) })?;
		}
		if let (Some(cert), Some(key)) = (
			non_empty(options.client_cert.as_deref()),
			non_empty(options.client_key.as_deref()),
		) {
			let cert = cstring(cert)?;
			let key = cstring(key)?;
			self.call("set client certificate", unsafe {
				sys::openconnect_set_client_cert(self.info, cert.as_ptr(), key.as_ptr())
			})?;
		}

		let gateway = cstring(&options.gateway_url)?;
		self.call("parse gateway URL", unsafe {
			sys::openconnect_parse_url(self.info, gateway.as_ptr())
		})?;

		// The cookie stands in for the whole authentication exchange.
		self.call("set session cookie", unsafe {
			sys::openconnect_set_cookie(self.info, cstring(&options.cookie)?.as_ptr())
		})?;
		// openconnect still POSTs ssl-vpn/getconfig.esp with the cookie, and
		// the server compares `computer` against the login. They must match.
		self.call("set local host name", unsafe {
			sys::openconnect_set_localname(self.info, cstring(&options.computer)?.as_ptr())
		})?;

		if let Some(wrapper) = non_empty(options.hip_script.as_deref()) {
			let wrapper = cstring(wrapper)?;
			// A 0 uid keeps the report generator running as the helper does.
			self.call("set up HIP report script", unsafe {
				sys::openconnect_setup_csd(self.info, 0, 1, wrapper.as_ptr())
			})?;
		}
		Ok(())
	}

	pub fn connect_tunnel(&mut self, options: &Options) -> Result<ConnectionInfo, String> {
		self.call("open CSTP connection", unsafe {
			sys::openconnect_make_cstp_connection(self.info)
		})?;
		let script = options
			.script
			.as_deref()
			.filter(|value| !value.is_empty())
			.map(str::to_owned)
			.or_else(|| find_vpnc_script().map(str::to_owned))
			.ok_or_else(|| {
				format!(
					"no vpnc-script found. Install the vpnc-scripts package, or set one in settings. Looked in: {}",
					VPNC_SCRIPT_LOCATIONS.join(", ")
				)
			})?;
		let script_arg = cstring(&script)?;
		self.call("set up tunnel device", unsafe {
			sys::openconnect_setup_tun_device(self.info, script_arg.as_ptr(), ptr::null())
		})?;
		let command_fd = unsafe { sys::openconnect_setup_cmd_pipe(self.info) };
		if command_fd < 0 {
			return Err(format!("create command pipe failed: {command_fd}"));
		}
		if let Ok(mut current) = self.cancel.0.lock() {
			if let Some(CancelFd::Auth(old_fd)) = current.replace(CancelFd::Command(command_fd)) {
				unsafe { libc::close(old_fd) };
			}
		}
		self.connection_info()
	}

	pub fn mainloop(&mut self) -> i32 {
		unsafe { sys::openconnect_mainloop(self.info, 300, 10) }
	}

	fn connection_info(&self) -> Result<ConnectionInfo, String> {
		let ifname = unsafe { string_from_ptr(sys::openconnect_get_ifname(self.info)) }.unwrap_or_default();
		let mut ip_info: *const sys::oc_ip_info = ptr::null();
		let result = unsafe {
			sys::openconnect_get_ip_info(self.info, &mut ip_info, ptr::null_mut(), ptr::null_mut())
		};
		if result != 0 || ip_info.is_null() {
			return Ok(ConnectionInfo { ifname, address: None, dns: Vec::new() });
		}
		let info = unsafe { &*ip_info };
		let address = unsafe { string_from_ptr(info.addr) };
		let dns = info
			.dns
			.iter()
			.filter_map(|value| unsafe { string_from_ptr(*value) })
			.collect();
		Ok(ConnectionInfo { ifname, address, dns })
	}

	fn call(&self, description: &str, result: i32) -> Result<(), String> {
		if result == 0 { Ok(()) } else { Err(format!("{description} failed: {result}")) }
	}
}

impl Drop for Vpn<'_> {
	fn drop(&mut self) {
		if let Ok(mut fd) = self.cancel.0.lock() {
			if let Some(CancelFd::Auth(value)) = fd.take() {
				unsafe { libc::close(value) };
			}
		}
		if !self.info.is_null() {
			unsafe { sys::openconnect_vpninfo_free(self.info) };
		}
		// The library owns the read end after set_cancel_fd().
		self.cancel_read_fd = -1;
		self.callback_context.vpninfo = ptr::null_mut();
	}
}

#[derive(Default)]
pub struct Options {
	/// `https://gateway[:port]` the cookie was issued for.
	pub gateway_url: String,
	/// Session cookie from `gp-auth`.
	pub cookie: String,
	/// Machine name sent at login; openconnect re-sends it with the cookie.
	pub computer: String,
	/// One of openconnect's own OS names: `linux-64`, `win`, `mac-intel`.
	pub reported_os: String,
	pub cafile: Option<String>,
	pub client_cert: Option<String>,
	pub client_key: Option<String>,
	/// Overrides vpnc-script discovery.
	pub script: Option<String>,
	/// HIP report generator, when the gateway asks for one.
	pub hip_script: Option<String>,
	pub log_level: i32,
}

pub struct ConnectionInfo {
	pub ifname: String,
	pub address: Option<String>,
	pub dns: Vec<String>,
}

/// Returns the library-owned hash for the certificate currently being validated.
///
/// # Safety
///
/// `vpninfo` must be a live libopenconnect instance and this must run during its
/// certificate-validation callback.
pub unsafe fn peer_cert_hash(vpninfo: *mut sys::openconnect_info) -> Option<String> {
	unsafe { string_from_ptr(sys::openconnect_get_peer_cert_hash(vpninfo)) }
}

/// Checks a previously stored certificate fingerprint against the current peer.
///
/// Accepts the `pin-sha256:`/`sha256:` forms, which is also what `gp-auth`
/// records, so one saved fingerprint covers login and tunnel alike.
///
/// # Safety
///
/// `vpninfo` must be a live libopenconnect instance in its validation callback.
pub unsafe fn peer_cert_matches(vpninfo: *mut sys::openconnect_info, fingerprint: &str) -> bool {
	let Ok(fingerprint) = cstring(fingerprint) else { return false };
	unsafe { sys::openconnect_check_peer_cert_hash(vpninfo, fingerprint.as_ptr()) == 0 }
}

/// Copies and frees the details for the certificate currently being validated.
///
/// # Safety
///
/// `vpninfo` must be a live libopenconnect instance in its validation callback.
pub unsafe fn peer_cert_details(vpninfo: *mut sys::openconnect_info) -> Option<String> {
	let details = unsafe { sys::openconnect_get_peer_cert_details(vpninfo) };
	if details.is_null() { return None; }
	let value = unsafe { string_from_ptr(details) };
	unsafe { sys::openconnect_free_cert_info(vpninfo, details.cast()) };
	value
}

fn non_empty(value: Option<&str>) -> Option<&str> {
	value.filter(|value| !value.is_empty())
}

fn cstring(value: &str) -> Result<CString, String> {
	CString::new(value).map_err(|_| "value contains an unsupported NUL byte".to_string())
}

unsafe fn string_from_ptr(value: *const libc::c_char) -> Option<String> {
	if value.is_null() { None } else { Some(unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned()) }
}

unsafe extern "C" fn validate_peer_cert(
	privdata: *mut libc::c_void,
	reason: *const libc::c_char,
) -> i32 {
	catch_unwind(AssertUnwindSafe(|| {
		let context = unsafe { &mut *privdata.cast::<CallbackContext>() };
		let callbacks = unsafe { &mut *context.callbacks };
		let reason = unsafe { string_from_ptr(reason) }.unwrap_or_else(|| "certificate rejected".into());
		callbacks.validate_peer_cert(context.vpninfo, &reason)
	}))
	.unwrap_or(-1)
}

unsafe extern "C" fn write_new_config(
	privdata: *mut libc::c_void,
	contents: *const libc::c_char,
	length: libc::c_int,
) -> i32 {
	catch_unwind(AssertUnwindSafe(|| {
		let context = unsafe { &mut *privdata.cast::<CallbackContext>() };
		let callbacks = unsafe { &mut *context.callbacks };
		let contents = if contents.is_null() || length < 0 {
			&[]
		} else {
			unsafe { std::slice::from_raw_parts(contents.cast::<u8>(), length as usize) }
		};
		callbacks.write_new_config(contents)
	}))
	.unwrap_or(-1)
}

/// Reached only if libopenconnect tries to authenticate, which the cookie is
/// meant to prevent. Failing is better than silently sending a blank form.
unsafe extern "C" fn refuse_auth_form(
	_privdata: *mut libc::c_void,
	_form: *mut sys::oc_auth_form,
) -> i32 {
	-1
}

#[unsafe(no_mangle)]
/// C trampoline target for libopenconnect's variadic progress callback.
///
/// # Safety
///
/// `privdata` must be the callback context installed by [`Vpn::new`] and
/// `message` must be a valid NUL-terminated C string.
pub unsafe extern "C" fn gp_rust_progress(
	privdata: *mut libc::c_void,
	level: libc::c_int,
	message: *const libc::c_char,
) {
	let _ = catch_unwind(AssertUnwindSafe(|| {
		if privdata.is_null() { return; }
		let context = unsafe { &mut *privdata.cast::<CallbackContext>() };
		let message = unsafe { string_from_ptr(message) }.unwrap_or_default();
		unsafe { (&mut *context.callbacks).progress(level, &message) };
	}));
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn script_discovery_only_returns_executables() {
		if let Some(script) = find_vpnc_script() {
			assert!(is_executable(Path::new(script)));
		}
	}

	#[test]
	fn a_directory_is_not_executable() {
		assert!(!is_executable(Path::new("/usr/share")));
	}
}
