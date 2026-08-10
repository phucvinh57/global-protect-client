//! `ssl-vpn/login.esp` - gateway login, which yields the tunnel cookie.

use std::net::ToSocketAddrs;

use crate::{
	credential::Credential,
	error::{Error, Result},
	http::{host_of, param, set_param, GpClient, Params},
	profile::Profile,
	xml,
};

/// A multi-factor prompt the gateway raised instead of logging us in.
#[derive(Debug, Clone)]
pub struct Challenge {
	pub message: String,
	/// Opaque token echoed back as `inputStr` with the one-time code.
	pub input_str: String,
}

#[derive(Debug)]
pub enum Login {
	Cookie(String),
	Challenge(Challenge),
}

pub struct LoginRequest<'a> {
	pub credential: &'a Credential,
	pub profile: &'a Profile,
	/// Gateway host, without a scheme.
	pub gateway_host: &'a str,
	/// Label from the portal's gateway list, when we came via a portal.
	pub gateway_name: Option<&'a str>,
	pub input_str: &'a str,
	/// One-time code, which replaces `passwd` rather than being appended to it.
	pub otp: Option<&'a str>,
	pub selected_manually: bool,
}

pub fn build_params(request: &LoginRequest<'_>) -> Params {
	let profile = request.profile;
	let mut body = request.credential.params(profile);

	body.push(param("prot", "https:"));
	body.push(param("jnlpReady", "jnlpReady"));
	body.push(param("ok", "Login"));
	body.push(param("direct", "yes"));
	body.push(param("ipv6-support", "yes"));
	body.push(param("clientVer", "4100"));
	body.push(param("clientos", profile.client_os.as_str()));
	body.push(param("computer", &profile.computer));
	body.push(param("inputStr", request.input_str));
	body.push(param("os-version", &profile.os_version));
	// The gateway wants the address it resolves to, not the name we dialled.
	body.push(param(
		"server",
		resolve_ipv4(request.gateway_host).unwrap_or_else(|| request.gateway_host.to_owned()),
	));
	body.push(param("host-id", &profile.host_id));
	body.push(param("serialno", &profile.serialno));
	body.push(param("preferred-ip", ""));
	body.push(param("preferred-ipv6", ""));
	body.push(param("clientgpversion", &profile.client_version));

	body.push(param("host", request.gateway_host));
	body.push(param("gw", request.gateway_host));
	body.push(param(
		"gateway-name",
		request.gateway_name.unwrap_or(request.gateway_host),
	));
	body.push(param("internal", "no"));
	body.push(param(
		"selection-type",
		if request.selected_manually { "manual" } else { "auto" },
	));
	body.push(param("client-ipv6", ""));
	if let Some(client_ip) = local_ipv4(request.gateway_host) {
		body.push(param("client-ip", client_ip));
	}

	if let Some(otp) = request.otp {
		set_param(&mut body, "passwd", otp);
	}
	body
}

pub fn login(client: &GpClient, gateway_url: &str, request: &LoginRequest<'_>) -> Result<Login> {
	let body = build_params(request);
	let url = format!("{gateway_url}/ssl-vpn/login.esp");
	let response = client.post_form(&url, &Vec::new(), &body)?;
	parse(&response, &request.profile.computer)
}

fn parse(body: &str, computer: &str) -> Result<Login> {
	if body.trim().is_empty() {
		return Err(Error::Parse("the gateway sent an empty login response".into()));
	}
	if let Some(challenge) = parse_challenge(body)? {
		return Ok(Login::Challenge(challenge));
	}
	xml::check_for_error(body)?;
	Ok(Login::Cookie(build_cookie(body, computer)?))
}

/// A challenge arrives as a JavaScript snippet wrapped in HTML, not as XML.
fn parse_challenge(body: &str) -> Result<Option<Challenge>> {
	if !body.contains("Challenge") {
		return Ok(None);
	}
	let message = between(body, "respMsg = \"", "\"")
		.or_else(|| between(body, "respmsg = \"", "\""))
		.unwrap_or_else(|| "Additional authentication is required".to_owned());
	let input_str = between(body, "inputStr.value = \"", "\"")
		.or_else(|| between(body, "inputstr.value = \"", "\""))
		.unwrap_or_default();
	Ok(Some(Challenge { message, input_str }))
}

fn between(haystack: &str, start: &str, end: &str) -> Option<String> {
	let rest = &haystack[haystack.find(start)? + start.len()..];
	let value = &rest[..rest.find(end)?];
	Some(value.trim().to_owned())
}

/// The tunnel cookie openconnect expects: the interesting `<argument>` slots of
/// the JNLP reply, re-encoded as a query string.
///
/// The indices are fixed by the protocol; the server sends around twenty
/// positional arguments and only these carry meaning.
fn build_cookie(body: &str, computer: &str) -> Result<String> {
	let document = xml::parse(body)?;
	let arguments: Vec<String> = document
		.descendants()
		.filter(|node| node.has_tag_name("argument"))
		.map(|node| node.text().unwrap_or_default().trim().to_owned())
		.collect();

	let at = |index: usize| {
		arguments
			.get(index)
			.map(String::as_str)
			.filter(|value| !value.is_empty() && *value != "(null)")
	};

	let mut fields: Vec<(&str, &str)> = Vec::new();
	fields.push((
		"authcookie",
		at(1).ok_or_else(|| Error::Parse("the gateway did not return an auth cookie".into()))?,
	));
	if let Some(value) = at(2) {
		fields.push(("persistent-cookie", value));
	}
	if let Some(value) = at(3) {
		fields.push(("portal", value));
	}
	fields.push((
		"user",
		at(4).ok_or_else(|| Error::Parse("the gateway did not return a user name".into()))?,
	));
	for (key, index) in [("domain", 7), ("preferred-ip", 15), ("preferred-ipv6", 18)] {
		if let Some(value) = at(index) {
			fields.push((key, value));
		}
	}
	fields.push(("computer", computer));

	Ok(fields
		.into_iter()
		.map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
		.collect::<Vec<_>>()
		.join("&"))
}

fn resolve_ipv4(host: &str) -> Option<String> {
	let host = host_of(host);
	let host = host.split(':').next().unwrap_or(&host);
	(host, 443)
		.to_socket_addrs()
		.ok()?
		.find(|address| address.is_ipv4())
		.map(|address| address.ip().to_string())
}

/// The local address that would be used to reach the gateway, which real
/// clients report so the server can apply source-based policy.
fn local_ipv4(host: &str) -> Option<String> {
	let host = host_of(host);
	let host = host.split(':').next().unwrap_or(&host);
	let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
	socket.connect((host, 443)).ok()?;
	let ip = socket.local_addr().ok()?.ip();
	ip.is_ipv4().then(|| ip.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::profile::ClientOs;

	fn profile() -> Profile {
		Profile {
			client_os: ClientOs::Linux,
			os_version: "Linux Ubuntu 24.04".into(),
			client_version: "6.3.3-619".into(),
			computer: "test-host".into(),
			host_id: "01234567-89ab-cdef-0123-456789abcdef".into(),
			serialno: "VMware-00".into(),
			user_agent: "PAN GlobalProtect".into(),
		}
	}

	fn request<'a>(
		credential: &'a Credential,
		profile: &'a Profile,
		otp: Option<&'a str>,
	) -> LoginRequest<'a> {
		LoginRequest {
			credential,
			profile,
			gateway_host: "nonexistent.invalid.test",
			gateway_name: Some("Ha Noi"),
			input_str: "",
			otp,
			selected_manually: false,
		}
	}

	fn find<'a>(params: &'a Params, key: &str) -> Option<&'a str> {
		params
			.iter()
			.find(|(name, _)| name == key)
			.map(|(_, value)| value.as_str())
	}

	#[test]
	fn the_body_carries_the_fields_openconnect_leaves_out() {
		let credential = Credential::password("alice", "secret");
		let profile = profile();
		let body = build_params(&request(&credential, &profile, None));
		assert_eq!(find(&body, "clientgpversion"), Some("6.3.3-619"));
		assert_eq!(find(&body, "host-id"), Some("01234567-89ab-cdef-0123-456789abcdef"));
		assert_eq!(find(&body, "serialno"), Some("VMware-00"));
		assert_eq!(find(&body, "computer"), Some("test-host"));
		assert_eq!(find(&body, "gw"), Some("nonexistent.invalid.test"));
		assert_eq!(find(&body, "gateway-name"), Some("Ha Noi"));
		assert_eq!(find(&body, "selection-type"), Some("auto"));
	}

	#[test]
	fn an_unresolvable_gateway_falls_back_to_its_host_name() {
		let credential = Credential::password("alice", "secret");
		let profile = profile();
		let body = build_params(&request(&credential, &profile, None));
		assert_eq!(find(&body, "server"), Some("nonexistent.invalid.test"));
	}

	#[test]
	fn an_otp_replaces_the_password_instead_of_being_appended() {
		let credential = Credential::password("alice", "secret");
		let profile = profile();
		let body = build_params(&request(&credential, &profile, Some("123456")));
		assert_eq!(find(&body, "passwd"), Some("123456"));
		assert_eq!(body.iter().filter(|(key, _)| key == "passwd").count(), 1);
	}

	#[test]
	fn a_challenge_response_is_recognised() {
		let body = r#"<html><body><script>
			var respStatus = "Challenge";
			var respMsg = "Enter the code from your token";
			document.getElementById('inputStr').value = "abc-123";
			inputStr.value = "abc-123";
			</script></body></html>"#;
		let Login::Challenge(challenge) = parse(body, "test-host").unwrap() else {
			panic!("expected a challenge");
		};
		assert_eq!(challenge.message, "Enter the code from your token");
		assert_eq!(challenge.input_str, "abc-123");
	}

	#[test]
	fn the_cookie_is_built_from_the_positional_arguments() {
		let mut arguments = vec![String::new(); 20];
		arguments[1] = "AUTHCOOKIE".into();
		arguments[2] = "PERSIST".into();
		arguments[3] = "GP-Portal".into();
		arguments[4] = "alice".into();
		arguments[7] = "example.com".into();
		arguments[15] = "10.1.2.3".into();
		let body = format!(
			"<jnlp><application-desc>{}</application-desc></jnlp>",
			arguments
				.iter()
				.map(|value| format!("<argument>{value}</argument>"))
				.collect::<String>()
		);
		let Login::Cookie(cookie) = parse(&body, "test-host").unwrap() else {
			panic!("expected a cookie");
		};
		assert_eq!(
			cookie,
			"authcookie=AUTHCOOKIE&persistent-cookie=PERSIST&portal=GP-Portal&user=alice\
			 &domain=example.com&preferred-ip=10.1.2.3&computer=test-host"
		);
	}

	#[test]
	fn a_reply_without_an_auth_cookie_is_an_error() {
		let body = "<jnlp><application-desc><argument>x</argument></application-desc></jnlp>";
		assert!(matches!(parse(body, "host"), Err(Error::Parse(_))));
	}

	#[test]
	fn cookie_values_are_url_encoded() {
		let mut arguments = vec![String::new(); 20];
		arguments[1] = "a+b/c=".into();
		arguments[4] = "alice@example.com".into();
		let body = format!(
			"<jnlp><application-desc>{}</application-desc></jnlp>",
			arguments
				.iter()
				.map(|value| format!("<argument>{value}</argument>"))
				.collect::<String>()
		);
		let Login::Cookie(cookie) = parse(&body, "host").unwrap() else {
			panic!("expected a cookie");
		};
		assert!(cookie.starts_with("authcookie=a%2Bb%2Fc%3D&"));
		assert!(cookie.contains("user=alice%40example.com"));
	}
}
