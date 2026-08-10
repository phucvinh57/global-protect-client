//! `global-protect/getconfig.esp` - portal login, which also returns the list
//! of gateways the user may connect to.

use crate::{
	credential::Credential,
	error::Result,
	http::{host_of, param, GpClient, Params},
	profile::Profile,
	xml,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gateway {
	/// Host name (sometimes `host:port`) used as the connection target.
	pub address: String,
	/// Human readable label shown in the UI.
	pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct PortalConfig {
	pub gateways: Vec<Gateway>,
	pub portal_name: Option<String>,
	pub user_auth_cookie: Option<String>,
	pub prelogon_user_auth_cookie: Option<String>,
	/// Version the portal reports; parroting it back keeps gateways from
	/// rejecting us as an obsolete client.
	pub portal_version: Option<String>,
}

pub fn build_params(credential: &Credential, profile: &Profile, base_url: &str, input_str: &str) -> Params {
	let mut body = credential.params(profile);
	body.push(param("inputStr", input_str));
	body.push(param("ok", "Login"));
	body.push(param("clientVer", "4100"));
	body.push(param("clientos", profile.client_os.as_str()));
	body.push(param("clientgpversion", &profile.client_version));
	body.push(param("computer", &profile.computer));
	body.push(param("os-version", &profile.os_version));
	body.push(param("host-id", &profile.host_id));
	body.push(param("ipv6-support", "yes"));
	body.push(param("serialno", &profile.serialno));
	body.push(param("csc-digest", ""));
	body.push(param("config-digest", ""));
	body.push(param(
		"csc-support",
		if profile.client_os.csc_support() { "yes" } else { "no" },
	));
	body.push(param("host", host_of(base_url)));
	body.push(param("swg-auth-token", "0"));
	body.push(param("swg-nonce", "0"));
	body
}

pub fn get_config(
	client: &GpClient,
	base_url: &str,
	credential: &Credential,
	profile: &Profile,
	input_str: &str,
) -> Result<PortalConfig> {
	let body = build_params(credential, profile, base_url, input_str);
	let url = format!("{base_url}/global-protect/getconfig.esp");
	let response = client.post_form(&url, &Vec::new(), &body)?;
	parse(&response)
}

fn parse(body: &str) -> Result<PortalConfig> {
	xml::check_for_error(body)?;
	let document = xml::parse(body)?;
	let root = document.root_element();

	let mut config = PortalConfig {
		portal_name: xml::text(root, "portal-name"),
		user_auth_cookie: cookie_value(xml::text(root, "portal-userauthcookie")),
		prelogon_user_auth_cookie: cookie_value(xml::text(root, "portal-prelogonuserauthcookie")),
		portal_version: xml::find(root, "policy").and_then(|policy| xml::child_text(policy, "version")),
		gateways: Vec::new(),
	};

	// policy/gateways/external/list/entry[@name]
	if let Some(list) = xml::find(root, "gateways")
		.and_then(|gateways| xml::find(gateways, "external"))
		.and_then(|external| xml::find(external, "list"))
	{
		for entry in list.children().filter(|child| child.has_tag_name("entry")) {
			let Some(address) = entry.attribute("name").map(str::trim).filter(|value| !value.is_empty()) else {
				continue;
			};
			let name = xml::child_text(entry, "description").unwrap_or_else(|| address.to_owned());
			config.gateways.push(Gateway {
				address: address.to_owned(),
				name,
			});
		}
	}

	Ok(config)
}

/// The portal writes `empty` (or nothing) when there is no cookie to reuse.
fn cookie_value(value: Option<String>) -> Option<String> {
	value.filter(|value| !value.is_empty() && value != "empty")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{credential::Credential, profile::ClientOs};

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

	#[test]
	fn identity_fields_openconnect_omits_are_present() {
		let credential = Credential::password("alice", "secret");
		let body = build_params(&credential, &profile(), "https://vpn.example.com", "");
		let find = |key: &str| {
			body.iter()
				.find(|(name, _)| name == key)
				.map(|(_, value)| value.as_str())
		};
		assert_eq!(find("host-id"), Some("01234567-89ab-cdef-0123-456789abcdef"));
		assert_eq!(find("serialno"), Some("VMware-00"));
		assert_eq!(find("clientgpversion"), Some("6.3.3-619"));
		assert_eq!(find("computer"), Some("test-host"));
		assert_eq!(find("host"), Some("vpn.example.com"));
		assert_eq!(find("csc-support"), Some("no"));
	}

	#[test]
	fn gateways_are_read_from_the_policy() {
		let body = r#"<policy>
			<portal-name>GP-Portal</portal-name>
			<version>6.2.3</version>
			<portal-userauthcookie>empty</portal-userauthcookie>
			<portal-prelogonuserauthcookie>abc123</portal-prelogonuserauthcookie>
			<gateways><external><list>
				<entry name="gw1.example.com"><description>Ha Noi</description></entry>
				<entry name="gw2.example.com:443"><description>Sai Gon</description></entry>
			</list></external></gateways>
		</policy>"#;
		let config = parse(body).unwrap();
		assert_eq!(config.gateways.len(), 2);
		assert_eq!(config.gateways[0].address, "gw1.example.com");
		assert_eq!(config.gateways[0].name, "Ha Noi");
		assert_eq!(config.gateways[1].address, "gw2.example.com:443");
		assert_eq!(config.portal_version.as_deref(), Some("6.2.3"));
		assert_eq!(config.user_auth_cookie, None);
		assert_eq!(config.prelogon_user_auth_cookie.as_deref(), Some("abc123"));
	}

	#[test]
	fn an_entry_without_a_description_falls_back_to_its_address() {
		let body = r#"<policy><gateways><external><list>
			<entry name="gw1.example.com"/>
		</list></external></gateways></policy>"#;
		let config = parse(body).unwrap();
		assert_eq!(config.gateways[0].name, "gw1.example.com");
	}
}
