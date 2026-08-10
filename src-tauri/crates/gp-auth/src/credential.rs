//! The credential half of a portal or gateway login body.

use zeroize::Zeroize;

use crate::{
	http::{param, Params},
	profile::Profile,
};

#[derive(Clone)]
pub enum Credential {
	/// Username and password typed by the user.
	Password { username: String, password: String },
	/// Cookies handed out by a successful portal login, reused against the
	/// gateway so the user is not prompted twice.
	PortalCookie {
		username: String,
		password: Option<String>,
		user_auth_cookie: Option<String>,
		prelogon_user_auth_cookie: Option<String>,
	},
}

impl Credential {
	pub fn password(username: impl Into<String>, password: impl Into<String>) -> Self {
		Credential::Password {
			username: username.into(),
			password: password.into(),
		}
	}

	pub fn username(&self) -> &str {
		match self {
			Credential::Password { username, .. } | Credential::PortalCookie { username, .. } => username,
		}
	}

	/// `user`, `passwd` and the cookie fields, in the order a real client sends
	/// them. Every field is always present - the server treats a missing key
	/// differently from an empty one.
	pub fn params(&self, profile: &Profile) -> Params {
		let (password, user_auth_cookie, prelogon_user_auth_cookie) = match self {
			Credential::Password { password, .. } => (password.clone(), String::new(), String::new()),
			Credential::PortalCookie {
				password,
				user_auth_cookie,
				prelogon_user_auth_cookie,
				..
			} => (
				password
					.clone()
					.unwrap_or_else(|| profile.client_os.saml_password().to_owned()),
				user_auth_cookie.clone().unwrap_or_else(|| "empty".into()),
				prelogon_user_auth_cookie.clone().unwrap_or_else(|| "empty".into()),
			),
		};
		vec![
			param("user", self.username()),
			param("passwd", password),
			param("prelogin-cookie", ""),
			param("portal-userauthcookie", user_auth_cookie),
			param("portal-prelogonuserauthcookie", prelogon_user_auth_cookie),
			param("token", ""),
		]
	}
}

/// Redacted, so a credential can never reach a log through `{:?}`.
impl std::fmt::Debug for Credential {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("Credential")
			.field("username", &self.username())
			.field("password", &"<redacted>")
			.finish()
	}
}

impl Zeroize for Credential {
	fn zeroize(&mut self) {
		match self {
			Credential::Password { password, .. } => password.zeroize(),
			Credential::PortalCookie { password, .. } => {
				if let Some(password) = password {
					password.zeroize();
				}
			}
		}
	}
}

impl Drop for Credential {
	fn drop(&mut self) {
		self.zeroize();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::profile::ClientOs;

	fn profile() -> Profile {
		Profile {
			client_os: ClientOs::Linux,
			os_version: "Linux".into(),
			client_version: "6.3.3-619".into(),
			computer: "host".into(),
			host_id: "id".into(),
			serialno: "serial".into(),
			user_agent: "PAN GlobalProtect".into(),
		}
	}

	fn find<'a>(params: &'a Params, key: &str) -> Option<&'a str> {
		params
			.iter()
			.find(|(name, _)| name == key)
			.map(|(_, value)| value.as_str())
	}

	#[test]
	fn a_password_login_sends_empty_cookie_fields() {
		let params = Credential::password("alice", "secret").params(&profile());
		assert_eq!(find(&params, "user"), Some("alice"));
		assert_eq!(find(&params, "passwd"), Some("secret"));
		assert_eq!(find(&params, "portal-userauthcookie"), Some(""));
	}

	#[test]
	fn a_cookie_login_without_a_password_uses_the_saml_placeholder() {
		let credential = Credential::PortalCookie {
			username: "alice".into(),
			password: None,
			user_auth_cookie: Some("cookie".into()),
			prelogon_user_auth_cookie: None,
		};
		let params = credential.params(&profile());
		assert_eq!(find(&params, "passwd"), Some("SAMLPASS"));
		assert_eq!(find(&params, "portal-userauthcookie"), Some("cookie"));
		assert_eq!(find(&params, "portal-prelogonuserauthcookie"), Some("empty"));
	}

	#[test]
	fn a_cookie_login_reuses_the_portal_password_when_there_is_one() {
		let credential = Credential::PortalCookie {
			username: "alice".into(),
			password: Some("secret".into()),
			user_auth_cookie: None,
			prelogon_user_auth_cookie: None,
		};
		assert_eq!(find(&credential.params(&profile()), "passwd"), Some("secret"));
	}
}
