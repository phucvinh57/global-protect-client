//! A GlobalProtect portal/gateway client that produces a tunnel cookie.
//!
//! libopenconnect can speak the GlobalProtect protocol itself, but the login it
//! sends identifies as `computer=localhost`, `os-version=linux-64`, no
//! `host-id`, no `serialno` and no `clientgpversion`. PAN-OS deployments that
//! enforce a client version or a device identity answer that with the
//! non-standard HTTP status 512, which openconnect surfaces only as
//! "Unexpected 512 result from server" because the actual cause travels in a
//! header it never reads.
//!
//! So this crate performs the whole login - prelogin, portal config, gateway
//! login, MFA challenge - as a real client would, and hands the resulting
//! cookie to `openconnect_set_cookie()`. openconnect is then used purely for
//! the tunnel.

mod credential;
mod error;
mod gateway;
mod http;
mod portal;
mod prelogin;
mod profile;
mod tls;
mod xml;

use std::sync::Arc;

use zeroize::Zeroize;

pub use credential::Credential;
pub use error::{Error, Result, ServerError};
pub use gateway::Challenge;
pub use http::{normalize_server, LogCallback};
pub use portal::Gateway;
pub use profile::{ClientOs, Profile};
pub use tls::{CertPrompt, TlsOptions, TrustCallback};

use http::{host_of, GpClient};

/// A question the login flow needs answered before it can continue.
pub enum Question<'a> {
	/// The gateway wants a one-time code.
	Challenge(&'a Challenge),
	/// The portal offers more than one gateway.
	Gateway(&'a [Gateway]),
}

/// The caller's answer. [`Answer::Cancelled`] aborts the login.
pub enum Answer {
	Otp(String),
	Gateway(String),
	Cancelled,
}

/// Answers [`Question`]s by prompting the user.
pub type Ask = Arc<dyn Fn(Question<'_>) -> Answer + Send + Sync>;

pub struct AuthRequest {
	/// Portal or gateway address as typed by the user.
	pub server: String,
	pub username: String,
	pub password: String,
	/// Code appended to the password on the first attempt, for portals that
	/// take both at once. A gateway challenge is answered through [`Ask`].
	pub otp: String,
	/// Skips gateway discovery and logs straight into this gateway.
	pub gateway: Option<String>,
	pub client_os: ClientOs,
	pub tls: TlsOptions,
}

/// What openconnect needs to bring the tunnel up.
pub struct Session {
	/// `https://gateway[:port]`, to hand to `openconnect_parse_url()`.
	pub gateway_url: String,
	pub gateway_name: String,
	/// The value for `openconnect_set_cookie()`.
	pub cookie: String,
	/// Identity that must match what we logged in with, because openconnect
	/// still sends its own `getconfig.esp` afterwards.
	pub profile: Profile,
	/// Gateways the portal offered, so the UI can remember them.
	pub gateways: Vec<Gateway>,
}

/// Logs in and returns the tunnel cookie.
///
/// Tries the address as a portal first and falls back to treating it as a
/// gateway, which is what the official client does.
pub fn authenticate(
	mut request: AuthRequest,
	trust: TrustCallback,
	log: LogCallback,
	ask: Ask,
) -> Result<Session> {
	let base_url = normalize_server(&request.server)?;
	let profile = Profile::detect(request.client_os);
	log(&format!(
		"Identifying as {} (client {}, computer {})",
		profile.client_os.as_str(),
		profile.client_version,
		profile.computer
	));
	let client = GpClient::new(&profile.user_agent, &request.tls, trust, log.clone())?;

	let password = if request.otp.is_empty() {
		request.password.clone()
	} else {
		format!("{}{}", request.password, request.otp)
	};
	let credential = Credential::password(&request.username, password);
	// The credential owns the only copy from here on, and zeroizes on drop.
	request.password.zeroize();
	request.otp.zeroize();

	// An explicit gateway, or an address that already names one, skips the
	// portal entirely.
	if let Some(gateway) = request.gateway.as_deref().filter(|value| !value.trim().is_empty()) {
		let gateway_url = normalize_server(gateway)?;
		log(&format!("Logging in to gateway {}", host_of(&gateway_url)));
		// Real clients always prelogin first, and some gateways need it to
		// establish the session the login then completes.
		let prelogin = prelogin::run(&client, &gateway_url, &profile, prelogin::Endpoint::Gateway)?;
		describe_prelogin(&log, &prelogin);
		return finish_at_gateway(&client, &gateway_url, None, &credential, &profile, &ask, true, Vec::new());
	}

	log(&format!("Contacting portal {}", host_of(&base_url)));
	let portal_config = match prelogin::run(&client, &base_url, &profile, prelogin::Endpoint::Portal)
		.inspect(|prelogin| describe_prelogin(&log, prelogin))
		.and_then(|_| portal::get_config(&client, &base_url, &credential, &profile, ""))
	{
		Ok(config) => config,
		Err(error) if is_not_a_portal(&error) => {
			log(&format!("{} is not a portal, treating it as a gateway", host_of(&base_url)));
			let prelogin = prelogin::run(&client, &base_url, &profile, prelogin::Endpoint::Gateway)?;
			describe_prelogin(&log, &prelogin);
			return finish_at_gateway(&client, &base_url, None, &credential, &profile, &ask, false, Vec::new());
		}
		Err(error) => return Err(error),
	};

	if let Some(name) = portal_config.portal_name.as_deref() {
		log(&format!(
			"Portal {name}{}",
			portal_config
				.portal_version
				.as_deref()
				.map(|version| format!(" reports GlobalProtect {version}"))
				.unwrap_or_default()
		));
	}

	if portal_config.gateways.is_empty() {
		return Err(Error::Auth(
			"the portal did not offer any gateway for this account".into(),
		));
	}
	log(&format!(
		"Portal offered {} gateway(s): {}",
		portal_config.gateways.len(),
		portal_config
			.gateways
			.iter()
			.map(|gateway| gateway.name.as_str())
			.collect::<Vec<_>>()
			.join(", ")
	));

	let (chosen, selected_manually) = if portal_config.gateways.len() == 1 {
		(portal_config.gateways[0].clone(), false)
	} else {
		match ask(Question::Gateway(&portal_config.gateways)) {
			Answer::Gateway(address) => {
				let chosen = portal_config
					.gateways
					.iter()
					.find(|gateway| gateway.address == address || gateway.name == address)
					.cloned()
					.ok_or_else(|| Error::Config(format!("unknown gateway {address}")))?;
				(chosen, true)
			}
			Answer::Cancelled => return Err(Error::Cancelled),
			Answer::Otp(_) => (portal_config.gateways[0].clone(), false),
		}
	};

	// Reuse the portal's cookies so the gateway does not re-prompt.
	let gateway_credential = Credential::PortalCookie {
		username: request.username.clone(),
		password: match &credential {
			Credential::Password { password, .. } => Some(password.clone()),
			Credential::PortalCookie { password, .. } => password.clone(),
		},
		user_auth_cookie: portal_config.user_auth_cookie.clone(),
		prelogon_user_auth_cookie: portal_config.prelogon_user_auth_cookie.clone(),
	};

	let gateway_url = normalize_server(&chosen.address)?;
	log(&format!("Logging in to gateway {} ({})", chosen.name, chosen.address));
	finish_at_gateway(
		&client,
		&gateway_url,
		Some(&chosen),
		&gateway_credential,
		&profile,
		&ask,
		selected_manually,
		portal_config.gateways.clone(),
	)
}

#[allow(clippy::too_many_arguments)]
fn finish_at_gateway(
	client: &GpClient,
	gateway_url: &str,
	gateway: Option<&Gateway>,
	credential: &Credential,
	profile: &Profile,
	ask: &Ask,
	selected_manually: bool,
	gateways: Vec<Gateway>,
) -> Result<Session> {
	let gateway_host = host_of(gateway_url);
	let mut input_str = String::new();
	let mut otp: Option<String> = None;

	// Each challenge produces one more attempt; the bound stops a
	// misconfigured server from looping forever.
	for _ in 0..5 {
		let request = gateway::LoginRequest {
			credential,
			profile,
			gateway_host: &gateway_host,
			gateway_name: gateway.map(|gateway| gateway.name.as_str()),
			input_str: &input_str,
			otp: otp.as_deref(),
			selected_manually,
		};
		match gateway::login(client, gateway_url, &request)? {
			gateway::Login::Cookie(cookie) => {
				return Ok(Session {
					gateway_url: gateway_url.to_owned(),
					gateway_name: gateway
						.map(|gateway| gateway.name.clone())
						.unwrap_or_else(|| gateway_host.clone()),
					cookie,
					profile: profile.clone(),
					gateways,
				})
			}
			gateway::Login::Challenge(challenge) => match ask(Question::Challenge(&challenge)) {
				Answer::Otp(code) => {
					input_str = challenge.input_str;
					otp = Some(code);
				}
				Answer::Cancelled => return Err(Error::Cancelled),
				Answer::Gateway(_) => return Err(Error::Cancelled),
			},
		}
	}
	Err(Error::Auth("too many authentication challenges".into()))
}

/// Echoes what the server said it wants, which is the quickest way to spot a
/// portal expecting something other than a username and password.
fn describe_prelogin(log: &LogCallback, prelogin: &prelogin::Prelogin) {
	if let Some(message) = prelogin.message.as_deref() {
		log(message);
	}
	let labels = [
		prelogin.username_label.as_deref(),
		prelogin.password_label.as_deref(),
	];
	let labels: Vec<&str> = labels.into_iter().flatten().collect();
	if !labels.is_empty() {
		log(&format!("Server asks for: {}", labels.join(", ")));
	}
	if let Some(region) = prelogin.region.as_deref() {
		log(&format!("Portal region: {region}"));
	}
}

/// Whether the failure means "this address is a gateway, not a portal".
fn is_not_a_portal(error: &Error) -> bool {
	match error {
		Error::Auth(message) => {
			let message = message.to_ascii_lowercase();
			message.contains("portal does not exist") || message.contains("not a portal")
		}
		Error::Server(server) => server.status == 404,
		_ => false,
	}
}
