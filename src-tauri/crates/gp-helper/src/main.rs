//! Privileged, JSON-lines bridge between the unprivileged GUI and the VPN.
//!
//! Authentication is handled by `gp-auth`, which speaks the GlobalProtect
//! portal and gateway protocol directly; libopenconnect receives the resulting
//! cookie and only builds the tunnel.

mod prompt;

use std::{
	io::{self, BufRead, Write},
	sync::{Arc, Mutex},
	thread::JoinHandle,
};

use gp_auth::{Answer, AuthRequest, ClientOs, Question, TlsOptions};
use openconnect::{sys, Callbacks, CancelHandle, Options, Vpn};
use serde::{Deserialize, Serialize};

use prompt::Prompt;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
	Connect(Box<ConnectRequest>),
	TrustCert,
	RejectCert,
	SubmitOtp { otp: String },
	SelectGateway { address: String },
	Disconnect,
}

#[derive(Debug, Deserialize)]
struct ConnectRequest {
	portal: String,
	username: String,
	password: String,
	#[serde(default)]
	otp: String,
	/// Skips portal discovery and logs straight into this gateway.
	#[serde(default)]
	gateway: Option<String>,
	/// `Linux`, `Windows` or `Mac`; portals that reject Linux clients usually
	/// accept a Windows one.
	#[serde(default)]
	client_os: Option<String>,
	#[serde(default)]
	cafile: Option<String>,
	#[serde(default)]
	client_cert: Option<String>,
	#[serde(default)]
	client_key: Option<String>,
	#[serde(default)]
	trusted_fingerprint: Option<String>,
	/// Overrides vpnc-script discovery.
	#[serde(default)]
	script: Option<String>,
	/// Submit a HIP report when the gateway asks for one.
	#[serde(default)]
	hip: bool,
	#[serde(default)]
	verbose: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
enum Event {
	State { state: &'static str },
	Log { level: &'static str, msg: String },
	CertUntrusted { fingerprint: String, details: String },
	MfaChallenge { message: String },
	Gateways { list: Vec<GatewayInfo>, selecting: bool },
	Connected { ifname: String, addr: Option<String>, dns: Vec<String>, gateway: String },
	Error { msg: String },
}

#[derive(Debug, Serialize, Clone)]
struct GatewayInfo {
	address: String,
	name: String,
}

#[derive(Clone)]
struct Emitter(Arc<Mutex<io::Stdout>>);

impl Emitter {
	fn emit(&self, event: Event) {
		let Ok(serialized) = serde_json::to_string(&event) else { return };
		if let Ok(mut output) = self.0.lock() {
			let _ = writeln!(output, "{serialized}");
			let _ = output.flush();
		}
	}

	fn log(&self, level: &'static str, message: impl Into<String>) {
		self.emit(Event::Log {
			level,
			msg: message.into(),
		});
	}
}

/// Everything a running connection can be asked about from the stdin thread.
#[derive(Default)]
struct Prompts {
	trust: Prompt<bool>,
	otp: Prompt<String>,
	gateway: Prompt<String>,
}

impl Prompts {
	fn cancel_all(&self) {
		self.trust.cancel();
		self.otp.cancel();
		self.gateway.cancel();
	}

	/// Clears a previous attempt's cancellation before a new connection.
	fn reset_all(&self) {
		self.trust.reset();
		self.otp.reset();
		self.gateway.reset();
	}
}

#[derive(Default)]
struct ConnectionControl(Mutex<Option<CancelHandle>>);

impl ConnectionControl {
	fn replace(&self, handle: CancelHandle) {
		if let Ok(mut current) = self.0.lock() {
			*current = Some(handle);
		}
	}

	fn cancel(&self) {
		if let Ok(current) = self.0.lock() {
			if let Some(handle) = current.as_ref() {
				handle.cancel();
			}
		}
	}

	fn clear(&self) {
		if let Ok(mut current) = self.0.lock() {
			*current = None;
		}
	}
}

/// Fingerprints the user has accepted, shared between the login (which uses
/// rustls) and the tunnel (which uses libopenconnect). Both record the value in
/// openconnect's `pin-sha256:` form, so one accepted certificate covers both.
#[derive(Clone, Default)]
struct TrustStore(Arc<Mutex<Vec<String>>>);

impl TrustStore {
	fn seed(&self, fingerprint: Option<String>) {
		if let (Ok(mut stored), Some(fingerprint)) = (self.0.lock(), fingerprint) {
			if !fingerprint.is_empty() {
				stored.push(fingerprint);
			}
		}
	}

	fn add(&self, fingerprint: String) {
		if let Ok(mut stored) = self.0.lock() {
			if !fingerprint.is_empty() && !stored.contains(&fingerprint) {
				stored.push(fingerprint);
			}
		}
	}

	fn all(&self) -> Vec<String> {
		self.0.lock().map(|stored| stored.clone()).unwrap_or_default()
	}
}

struct TunnelCallbacks {
	emitter: Emitter,
	prompts: Arc<Prompts>,
	trusted: TrustStore,
}

impl Callbacks for TunnelCallbacks {
	fn progress(&mut self, level: i32, message: &str) {
		let level = if level <= sys::PRG_ERR as i32 { "error" } else { "info" };
		self.emitter.log(level, message.trim());
	}

	fn validate_peer_cert(&mut self, vpninfo: *mut sys::openconnect_info, reason: &str) -> i32 {
		if vpninfo.is_null() {
			return -1;
		}
		for fingerprint in self.trusted.all() {
			if unsafe { openconnect::peer_cert_matches(vpninfo, &fingerprint) } {
				return 0;
			}
		}
		let fingerprint = unsafe { openconnect::peer_cert_hash(vpninfo) }.unwrap_or_default();
		let certificate = unsafe { openconnect::peer_cert_details(vpninfo) }.unwrap_or_default();
		let details = if certificate.is_empty() {
			reason.to_owned()
		} else {
			format!("{reason}\n\n{certificate}")
		};
		self.emitter.emit(Event::CertUntrusted {
			fingerprint: fingerprint.clone(),
			details,
		});
		if self.prompts.trust.wait().unwrap_or(false) {
			self.trusted.add(fingerprint);
			0
		} else {
			1
		}
	}
}

fn run_connection(
	request: ConnectRequest,
	emitter: Emitter,
	prompts: Arc<Prompts>,
	control: Arc<ConnectionControl>,
) {
	prompts.reset_all();
	let trusted = TrustStore::default();
	trusted.seed(request.trusted_fingerprint.clone());

	let result = (|| -> Result<(), String> {
		emitter.emit(Event::State { state: "authenticating" });
		let session = authenticate(&request, &emitter, &prompts, &trusted)?;

		emitter.emit(Event::State { state: "connecting" });
		let hip_script = request.hip.then(openconnect::find_hip_script).flatten();
		if request.hip && hip_script.is_none() {
			emitter.log(
				"error",
				"HIP reporting was requested but no hipreport.sh was found; continuing without it",
			);
		}
		let options = Options {
			gateway_url: session.gateway_url.clone(),
			cookie: session.cookie.clone(),
			computer: session.profile.computer.clone(),
			reported_os: session.profile.client_os.openconnect_os().to_owned(),
			cafile: request.cafile.clone(),
			client_cert: request.client_cert.clone(),
			client_key: request.client_key.clone(),
			script: request.script.clone(),
			hip_script: hip_script.map(str::to_owned),
			log_level: if request.verbose { sys::PRG_TRACE } else { sys::PRG_INFO } as i32,
		};

		let mut callbacks = TunnelCallbacks {
			emitter: emitter.clone(),
			prompts: prompts.clone(),
			trusted: trusted.clone(),
		};
		let mut vpn = Vpn::new(&mut callbacks, &session.profile.user_agent)?;
		control.replace(vpn.cancel_handle());
		vpn.configure(&options)?;
		let connection = vpn.connect_tunnel(&options)?;
		emitter.emit(Event::Connected {
			ifname: connection.ifname,
			addr: connection.address,
			dns: connection.dns,
			gateway: session.gateway_name.clone(),
		});
		emitter.emit(Event::State { state: "connected" });
		let exit = vpn.mainloop();
		if exit != 0 {
			emitter.log("info", format!("VPN loop ended: {exit}"));
		}
		Ok(())
	})();

	control.clear();
	prompts.cancel_all();
	if let Err(message) = result {
		emitter.emit(Event::Error { msg: message });
	}
	emitter.emit(Event::State { state: "disconnected" });
}

fn authenticate(
	request: &ConnectRequest,
	emitter: &Emitter,
	prompts: &Arc<Prompts>,
	trusted: &TrustStore,
) -> Result<gp_auth::Session, String> {
	let auth_request = AuthRequest {
		server: request.portal.clone(),
		username: request.username.clone(),
		password: request.password.clone(),
		otp: request.otp.clone(),
		gateway: request.gateway.clone(),
		client_os: ClientOs::parse(request.client_os.as_deref().unwrap_or_default()),
		tls: TlsOptions {
			cafile: request.cafile.clone(),
			client_cert: request.client_cert.clone(),
			client_key: request.client_key.clone(),
			trusted_fingerprint: request.trusted_fingerprint.clone(),
		},
	};

	let trust_emitter = emitter.clone();
	let trust_prompts = prompts.clone();
	let trust_store = trusted.clone();
	let trust = Arc::new(move |prompt: gp_auth::CertPrompt| {
		trust_emitter.emit(Event::CertUntrusted {
			fingerprint: prompt.fingerprint.clone(),
			details: prompt.details,
		});
		let accepted = trust_prompts.trust.wait().unwrap_or(false);
		if accepted {
			trust_store.add(prompt.fingerprint);
		}
		accepted
	});

	let log_emitter = emitter.clone();
	let log = Arc::new(move |message: &str| log_emitter.log("info", message));

	let ask_emitter = emitter.clone();
	let ask_prompts = prompts.clone();
	let ask = Arc::new(move |question: Question<'_>| match question {
		Question::Challenge(challenge) => {
			ask_emitter.emit(Event::MfaChallenge {
				message: challenge.message.clone(),
			});
			match ask_prompts.otp.wait() {
				Some(otp) => Answer::Otp(otp),
				None => Answer::Cancelled,
			}
		}
		Question::Gateway(gateways) => {
			ask_emitter.emit(Event::Gateways {
				list: gateways.iter().map(gateway_info).collect(),
				selecting: true,
			});
			match ask_prompts.gateway.wait() {
				Some(address) => Answer::Gateway(address),
				None => Answer::Cancelled,
			}
		}
	});

	let session = gp_auth::authenticate(auth_request, trust, log, ask).map_err(|error| error.to_string())?;
	if !session.gateways.is_empty() {
		// Sent again once the choice is settled, so the UI can remember it.
		emitter.emit(Event::Gateways {
			list: session.gateways.iter().map(gateway_info).collect(),
			selecting: false,
		});
	}
	emitter.log(
		"info",
		format!("Authenticated against gateway {}", session.gateway_name),
	);
	Ok(session)
}

fn gateway_info(gateway: &gp_auth::Gateway) -> GatewayInfo {
	GatewayInfo {
		address: gateway.address.clone(),
		name: gateway.name.clone(),
	}
}

fn main() {
	let emitter = Emitter(Arc::new(Mutex::new(io::stdout())));
	let prompts = Arc::new(Prompts::default());
	let control = Arc::new(ConnectionControl::default());
	let stdin = io::stdin();
	let mut worker: Option<JoinHandle<()>> = None;

	for line in stdin.lock().lines() {
		let Ok(line) = line else { break };
		let command = match serde_json::from_str::<Command>(&line) {
			Ok(command) => command,
			Err(error) => {
				emitter.emit(Event::Error {
					msg: format!("Invalid helper command: {error}"),
				});
				continue;
			}
		};
		match command {
			Command::Connect(request) => {
				if worker.as_ref().is_some_and(|thread| !thread.is_finished()) {
					emitter.emit(Event::Error {
						msg: "A VPN connection is already active".into(),
					});
					continue;
				}
				if let Some(thread) = worker.take() {
					let _ = thread.join();
				}
				let (worker_emitter, worker_prompts, worker_control) =
					(emitter.clone(), prompts.clone(), control.clone());
				worker = Some(std::thread::spawn(move || {
					run_connection(*request, worker_emitter, worker_prompts, worker_control);
				}));
			}
			Command::TrustCert => prompts.trust.answer(true),
			Command::RejectCert => prompts.trust.answer(false),
			Command::SubmitOtp { otp } => prompts.otp.answer(otp),
			Command::SelectGateway { address } => prompts.gateway.answer(address),
			Command::Disconnect => {
				emitter.emit(Event::State { state: "disconnecting" });
				prompts.cancel_all();
				control.cancel();
				if let Some(thread) = worker.take() {
					let _ = thread.join();
				}
				emitter.emit(Event::State { state: "disconnected" });
				break;
			}
		}
	}
	prompts.cancel_all();
	control.cancel();
	if let Some(thread) = worker {
		let _ = thread.join();
	}
}
