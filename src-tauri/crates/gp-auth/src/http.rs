//! The GlobalProtect HTTP surface: one client, and one way of reading replies.

use std::{sync::Arc, time::Duration};

use reqwest::blocking::{Client, Response};

use crate::{
	error::{Error, Result, ServerError},
	tls::{self, TlsOptions, TrustCallback},
};

/// Header PAN-OS uses to explain a rejection. openconnect never reads it, which
/// is why a bare "512" is all this app used to be able to report.
const REASON_HEADER: &str = "x-private-pan-globalprotect";

/// Form parameters, ordered the way a real client sends them.
pub type Params = Vec<(String, String)>;

/// Adds a parameter, replacing an earlier one with the same name.
pub fn set_param(params: &mut Params, key: &str, value: impl Into<String>) {
	let value = value.into();
	match params.iter_mut().find(|(name, _)| name == key) {
		Some(entry) => entry.1 = value,
		None => params.push((key.to_owned(), value)),
	}
}

pub fn param(key: &str, value: impl Into<String>) -> (String, String) {
	(key.to_owned(), value.into())
}

/// Emits one line of progress, with anything secret already removed.
pub type LogCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct GpClient {
	client: Client,
	log: LogCallback,
}

impl GpClient {
	pub fn new(user_agent: &str, options: &TlsOptions, trust: TrustCallback, log: LogCallback) -> Result<Self> {
		let config = tls::client_config(options, trust)?;
		let client = Client::builder()
			.user_agent(user_agent)
			.timeout(Duration::from_secs(60))
			.tls_backend_preconfigured(config)
			.build()?;
		Ok(Self { client, log })
	}

	pub fn log(&self, message: impl AsRef<str>) {
		(self.log)(message.as_ref());
	}

	/// POSTs a form and returns the body, turning any non-2xx reply into a
	/// [`ServerError`] that carries the status, the reason header and the body.
	pub fn post_form(&self, url: &str, query: &Params, body: &Params) -> Result<String> {
		self.log(format!(
			"POST {url} [{}]",
			body.iter()
				.chain(query.iter())
				.map(|(key, _)| key.as_str())
				.collect::<Vec<_>>()
				.join(", ")
		));
		let response = self.client.post(url).query(query).form(body).send()?;
		self.read(url, response)
	}

	fn read(&self, url: &str, response: Response) -> Result<String> {
		let status = response.status();
		let reason = response
			.headers()
			.get(REASON_HEADER)
			.and_then(|value| value.to_str().ok())
			.unwrap_or_default()
			.to_owned();
		if !status.is_success() {
			let body = response.text().unwrap_or_default();
			self.log(format!(
				"{url} -> HTTP {} ({})",
				status.as_u16(),
				if reason.is_empty() { "no reason header" } else { &reason }
			));
			return Err(ServerError {
				status: status.as_u16(),
				reason,
				body,
			}
			.into());
		}
		let body = response.text()?;
		self.log(format!("{url} -> HTTP {} ({} bytes)", status.as_u16(), body.len()));
		Ok(body)
	}
}

/// Normalises what the user typed into an `https://host[:port]` base URL.
pub fn normalize_server(server: &str) -> Result<String> {
	let server = server.trim().trim_end_matches('/');
	if server.is_empty() {
		return Err(Error::Config("no portal address was given".into()));
	}
	let with_scheme = if server.contains("://") {
		server.to_owned()
	} else {
		format!("https://{server}")
	};
	let parsed = reqwest::Url::parse(&with_scheme)
		.map_err(|error| Error::Config(format!("{server} is not a valid address: {error}")))?;
	if parsed.scheme() != "https" {
		return Err(Error::Config("the portal address must use https".into()));
	}
	let host = parsed
		.host_str()
		.ok_or_else(|| Error::Config(format!("{server} has no host name")))?;
	Ok(match parsed.port() {
		Some(port) => format!("https://{host}:{port}"),
		None => format!("https://{host}"),
	})
}

/// The `host`/`gw` form fields want the bare host, without the scheme.
pub fn host_of(base_url: &str) -> String {
	base_url
		.trim_start_matches("https://")
		.trim_start_matches("http://")
		.trim_end_matches('/')
		.to_owned()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn bare_host_gets_https() {
		assert_eq!(normalize_server("vpn.example.com").unwrap(), "https://vpn.example.com");
	}

	#[test]
	fn trailing_slashes_and_paths_are_dropped() {
		assert_eq!(
			normalize_server("https://vpn.example.com/global-protect/").unwrap(),
			"https://vpn.example.com"
		);
	}

	#[test]
	fn a_custom_port_is_kept() {
		assert_eq!(
			normalize_server("https://vpn.example.com:8443").unwrap(),
			"https://vpn.example.com:8443"
		);
	}

	#[test]
	fn plain_http_is_refused() {
		assert!(normalize_server("http://vpn.example.com").is_err());
	}

	#[test]
	fn set_param_replaces_rather_than_duplicates() {
		let mut params = vec![param("passwd", "secret"), param("user", "alice")];
		set_param(&mut params, "passwd", "123456");
		assert_eq!(params.len(), 2);
		assert_eq!(params[0].1, "123456");
	}
}
