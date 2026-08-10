//! `prelogin.esp` - asks the portal or gateway which credentials it wants.

use crate::{
	error::{Error, Result},
	http::{param, GpClient, Params},
	profile::Profile,
	xml,
};

/// `default-browser` values a real client sends for an embedded login form.
const PORTAL_EMBEDDED_BROWSER: &str = "-10";
const GATEWAY_EMBEDDED_BROWSER: &str = "0";
/// `{"cas_embedded_browser":"yes"}`, base64 encoded, exactly as sent by the
/// official client.
const CAS_DATA: &str = "eyJjYXNfZW1iZWRkZWRfYnJvd3NlciI6InllcyJ9";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
	Portal,
	Gateway,
}

impl Endpoint {
	fn path(self) -> &'static str {
		match self {
			Endpoint::Portal => "global-protect/prelogin.esp",
			Endpoint::Gateway => "ssl-vpn/prelogin.esp",
		}
	}

	fn default_browser(self) -> &'static str {
		match self {
			Endpoint::Portal => PORTAL_EMBEDDED_BROWSER,
			Endpoint::Gateway => GATEWAY_EMBEDDED_BROWSER,
		}
	}
}

#[derive(Debug, Clone, Default)]
pub struct Prelogin {
	pub message: Option<String>,
	pub username_label: Option<String>,
	pub password_label: Option<String>,
	pub region: Option<String>,
}

/// Builds the prelogin parameters.
///
/// Windows clients put them in the query string, the others in the body; the
/// split matters because some portals only look in one place.
pub fn build_params(profile: &Profile, endpoint: Endpoint) -> (Params, Params) {
	let params = vec![
		param("tmp", "tmp"),
		param("clientVer", "4100"),
		param("clientos", profile.client_os.as_str()),
		param("os-version", &profile.os_version),
		param("host-id", &profile.host_id),
		param("ipv6-support", "yes"),
		param("default-browser", endpoint.default_browser()),
		param("cas-support", "yes"),
		param("data", CAS_DATA),
	];
	let mut query = Vec::new();
	if profile.client_os.kerberos_support_in_query() {
		query.push(param("kerberos-support", "yes"));
	}
	if profile.client_os.prelogin_params_in_query() {
		query.extend(params);
		(query, Vec::new())
	} else {
		(query, params)
	}
}

pub fn run(client: &GpClient, base_url: &str, profile: &Profile, endpoint: Endpoint) -> Result<Prelogin> {
	let (query, body) = build_params(profile, endpoint);
	let url = format!("{base_url}/{}", endpoint.path());
	let response = client.post_form(&url, &query, &body)?;
	parse(&response)
}

fn parse(body: &str) -> Result<Prelogin> {
	xml::check_for_error(body)?;
	let document = xml::parse(body)?;
	let root = document.root_element();

	if xml::text(root, "saml-request").is_some() || xml::text(root, "saml-auth-method").is_some() {
		return Err(Error::Unsupported(
			"this portal uses SAML/single sign-on, which this client cannot do yet".into(),
		));
	}

	if let Some(status) = xml::text(root, "status") {
		if !status.eq_ignore_ascii_case("success") {
			let message = xml::text(root, "msg")
				.or_else(|| xml::text(root, "authentication-message"))
				.unwrap_or(status);
			return Err(Error::Auth(message));
		}
	}

	Ok(Prelogin {
		message: xml::text(root, "authentication-message"),
		username_label: xml::text(root, "username-label"),
		password_label: xml::text(root, "password-label"),
		region: xml::text(root, "region"),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::profile::ClientOs;

	fn profile(client_os: ClientOs) -> Profile {
		Profile {
			client_os,
			os_version: "Linux Ubuntu 24.04".into(),
			client_version: "6.3.3-619".into(),
			computer: "test-host".into(),
			host_id: "01234567-89ab-cdef-0123-456789abcdef".into(),
			serialno: "VMware-00".into(),
			user_agent: "PAN GlobalProtect".into(),
		}
	}

	#[test]
	fn linux_sends_parameters_in_the_body() {
		let (query, body) = build_params(&profile(ClientOs::Linux), Endpoint::Portal);
		assert!(query.is_empty());
		assert!(body.iter().any(|(key, value)| key == "host-id" && !value.is_empty()));
		assert!(body.iter().any(|(key, value)| key == "default-browser" && value == "-10"));
	}

	#[test]
	fn windows_sends_parameters_in_the_query_with_kerberos() {
		let (query, body) = build_params(&profile(ClientOs::Windows), Endpoint::Gateway);
		assert!(body.is_empty());
		assert!(query.iter().any(|(key, _)| key == "kerberos-support"));
		assert!(query.iter().any(|(key, value)| key == "default-browser" && value == "0"));
	}

	#[test]
	fn labels_are_read_from_the_response() {
		let body = r#"<prelogin-response><status>Success</status>
			<authentication-message>Enter login credentials</authentication-message>
			<username-label>Username</username-label>
			<password-label>Password</password-label>
			<region>VN</region></prelogin-response>"#;
		let prelogin = parse(body).unwrap();
		assert_eq!(prelogin.message.as_deref(), Some("Enter login credentials"));
		assert_eq!(prelogin.password_label.as_deref(), Some("Password"));
		assert_eq!(prelogin.region.as_deref(), Some("VN"));
	}

	#[test]
	fn a_saml_portal_is_refused_with_a_clear_message() {
		let body = r#"<prelogin-response><status>Success</status>
			<saml-auth-method>REDIRECT</saml-auth-method>
			<saml-request>PHNhbWw+</saml-request></prelogin-response>"#;
		let error = parse(body).unwrap_err();
		assert!(matches!(error, Error::Unsupported(message) if message.contains("SAML")));
	}
}
