use std::fmt;

/// A non-2xx reply from a GlobalProtect endpoint.
///
/// PAN-OS answers a rejected client with non-standard statuses (512 "Custom
/// error", 513 "Valid client certificate is required") and puts the human
/// readable cause in the `x-private-pan-globalprotect` header rather than the
/// body, so both are captured here.
#[derive(Debug, Clone)]
pub struct ServerError {
	pub status: u16,
	pub reason: String,
	pub body: String,
}

impl fmt::Display for ServerError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "server returned HTTP {}", self.status)?;
		if !self.reason.is_empty() {
			write!(formatter, " ({})", self.reason)?;
		}
		if self.status == 512 && self.reason.is_empty() {
			write!(
				formatter,
				" - the portal rejected this client. Try a different reported client OS."
			)?;
		}
		let body = self.body.trim();
		if !body.is_empty() {
			write!(formatter, ": {}", truncate(body, 400))?;
		}
		Ok(())
	}
}

impl std::error::Error for ServerError {}

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("{0}")]
	Config(String),
	#[error("could not reach the portal: {0}")]
	Network(String),
	#[error("{0}")]
	Server(#[from] ServerError),
	#[error("could not understand the server response: {0}")]
	Parse(String),
	#[error("authentication failed: {0}")]
	Auth(String),
	#[error("{0}")]
	Unsupported(String),
	#[error("cancelled")]
	Cancelled,
}

impl From<reqwest::Error> for Error {
	fn from(error: reqwest::Error) -> Self {
		Error::Network(error.to_string())
	}
}

impl From<roxmltree::Error> for Error {
	fn from(error: roxmltree::Error) -> Self {
		Error::Parse(error.to_string())
	}
}

pub type Result<T> = std::result::Result<T, Error>;

fn truncate(value: &str, limit: usize) -> String {
	if value.chars().count() <= limit {
		return value.to_owned();
	}
	value.chars().take(limit).collect::<String>() + "..."
}
