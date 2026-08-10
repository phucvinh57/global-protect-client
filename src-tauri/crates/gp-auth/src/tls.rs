//! Certificate verification that mirrors what libopenconnect does for the
//! tunnel, so a fingerprint accepted here is also accepted by openconnect.

use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rustls::{
	client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
	crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider},
	pki_types::{CertificateDer, ServerName, UnixTime},
	DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// What the user is shown when a certificate does not validate.
#[derive(Debug, Clone)]
pub struct CertPrompt {
	/// `pin-sha256:<base64>`, the same shape `openconnect_get_peer_cert_hash()`
	/// produces, so one stored value serves both this crate and the tunnel.
	pub fingerprint: String,
	pub details: String,
}

/// Answers a certificate prompt; returning false aborts the connection.
pub type TrustCallback = Arc<dyn Fn(CertPrompt) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct TlsOptions {
	/// Extra CA bundle, as configured in settings.
	pub cafile: Option<String>,
	pub client_cert: Option<String>,
	pub client_key: Option<String>,
	/// Fingerprint the user accepted previously.
	pub trusted_fingerprint: Option<String>,
}

/// Installs the process-wide crypto provider exactly once.
///
/// reqwest is built without a default provider so that `ring` is used instead of
/// `aws-lc-rs`, which would drag in a cmake toolchain.
pub fn install_crypto_provider() {
	if CryptoProvider::get_default().is_none() {
		let _ = ring::default_provider().install_default();
	}
}

/// SHA-256 over the SubjectPublicKeyInfo, which is what openconnect hashes for
/// its `sha256:`/`pin-sha256:` forms - not the full certificate.
fn public_key_fingerprint(certificate: &CertificateDer<'_>) -> Option<String> {
	let (_, parsed) = x509_parser::parse_x509_certificate(certificate.as_ref()).ok()?;
	let digest = Sha256::digest(parsed.public_key().raw);
	Some(format!("pin-sha256:{}", BASE64.encode(digest)))
}

fn certificate_details(certificate: &CertificateDer<'_>) -> String {
	let Ok((_, parsed)) = x509_parser::parse_x509_certificate(certificate.as_ref()) else {
		return String::new();
	};
	format!(
		"Subject: {}\nIssuer: {}\nValid: {} - {}",
		parsed.subject(),
		parsed.issuer(),
		parsed.validity().not_before,
		parsed.validity().not_after
	)
}

struct PinningVerifier {
	inner: Arc<rustls::client::WebPkiServerVerifier>,
	accepted: Mutex<Option<String>>,
	prompt: TrustCallback,
}

/// `ServerCertVerifier` requires `Debug`, and a callback has none.
impl std::fmt::Debug for PinningVerifier {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("PinningVerifier").finish_non_exhaustive()
	}
}

impl ServerCertVerifier for PinningVerifier {
	fn verify_server_cert(
		&self,
		end_entity: &CertificateDer<'_>,
		intermediates: &[CertificateDer<'_>],
		server_name: &ServerName<'_>,
		ocsp_response: &[u8],
		now: UnixTime,
	) -> std::result::Result<ServerCertVerified, rustls::Error> {
		let fingerprint = public_key_fingerprint(end_entity);
		if let (Ok(accepted), Some(fingerprint)) = (self.accepted.lock(), fingerprint.as_deref()) {
			if accepted.as_deref() == Some(fingerprint) {
				return Ok(ServerCertVerified::assertion());
			}
		}
		let rejection = match self
			.inner
			.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
		{
			Ok(verified) => return Ok(verified),
			Err(rejection) => rejection,
		};
		let Some(fingerprint) = fingerprint else {
			return Err(rejection);
		};
		let details = certificate_details(end_entity);
		let details = if details.is_empty() {
			rejection.to_string()
		} else {
			format!("{rejection}\n\n{details}")
		};
		if !(self.prompt)(CertPrompt {
			fingerprint: fingerprint.clone(),
			details,
		}) {
			return Err(rejection);
		}
		if let Ok(mut accepted) = self.accepted.lock() {
			*accepted = Some(fingerprint);
		}
		Ok(ServerCertVerified::assertion())
	}

	fn verify_tls12_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
		verify_tls12_signature(message, cert, dss, &ring::default_provider().signature_verification_algorithms)
	}

	fn verify_tls13_signature(
		&self,
		message: &[u8],
		cert: &CertificateDer<'_>,
		dss: &DigitallySignedStruct,
	) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
		verify_tls13_signature(message, cert, dss, &ring::default_provider().signature_verification_algorithms)
	}

	fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
		ring::default_provider()
			.signature_verification_algorithms
			.supported_schemes()
	}
}

/// Builds the rustls configuration handed to reqwest.
pub fn client_config(options: &TlsOptions, prompt: TrustCallback) -> Result<rustls::ClientConfig> {
	install_crypto_provider();

	let mut roots = RootCertStore::empty();
	// The system store, so corporate CAs already installed on the machine work
	// without also naming them in settings.
	for certificate in rustls_native_certs::load_native_certs().certs {
		let _ = roots.add(certificate);
	}
	if let Some(path) = options.cafile.as_deref().filter(|value| !value.is_empty()) {
		let file = std::fs::File::open(path)
			.map_err(|error| Error::Config(format!("could not open CA file {path}: {error}")))?;
		let mut reader = std::io::BufReader::new(file);
		for certificate in rustls_pemfile_certs(&mut reader)? {
			roots
				.add(certificate)
				.map_err(|error| Error::Config(format!("invalid certificate in {path}: {error}")))?;
		}
	}

	let inner = rustls::client::WebPkiServerVerifier::builder_with_provider(
		Arc::new(roots),
		Arc::new(ring::default_provider()),
	)
	.build()
	.map_err(|error| Error::Config(format!("could not build the certificate verifier: {error}")))?;

	let verifier = Arc::new(PinningVerifier {
		inner,
		accepted: Mutex::new(options.trusted_fingerprint.clone()),
		prompt,
	});

	let builder = rustls::ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
		.with_safe_default_protocol_versions()
		.map_err(|error| Error::Config(error.to_string()))?
		.dangerous()
		.with_custom_certificate_verifier(verifier);

	let config = match client_identity(options)? {
		Some((chain, key)) => builder
			.with_client_auth_cert(chain, key)
			.map_err(|error| Error::Config(format!("invalid client certificate: {error}")))?,
		None => builder.with_no_client_auth(),
	};
	Ok(config)
}

fn client_identity(
	options: &TlsOptions,
) -> Result<Option<(Vec<CertificateDer<'static>>, rustls::pki_types::PrivateKeyDer<'static>)>> {
	let (Some(cert_path), Some(key_path)) = (
		options.client_cert.as_deref().filter(|value| !value.is_empty()),
		options.client_key.as_deref().filter(|value| !value.is_empty()),
	) else {
		return Ok(None);
	};
	let cert_file = std::fs::File::open(cert_path)
		.map_err(|error| Error::Config(format!("could not open {cert_path}: {error}")))?;
	let chain = rustls_pemfile_certs(&mut std::io::BufReader::new(cert_file))?;
	if chain.is_empty() {
		return Err(Error::Config(format!("no certificate found in {cert_path}")));
	}
	let key_file = std::fs::File::open(key_path)
		.map_err(|error| Error::Config(format!("could not open {key_path}: {error}")))?;
	let key = rustls_pemfile_key(&mut std::io::BufReader::new(key_file))?
		.ok_or_else(|| Error::Config(format!("no private key found in {key_path}")))?;
	Ok(Some((chain, key)))
}

fn rustls_pemfile_certs(
	reader: &mut dyn std::io::BufRead,
) -> Result<Vec<CertificateDer<'static>>> {
	rustls_pemfile::certs(reader)
		.collect::<std::result::Result<Vec<_>, _>>()
		.map_err(|error| Error::Config(format!("could not read certificates: {error}")))
}

fn rustls_pemfile_key(
	reader: &mut dyn std::io::BufRead,
) -> Result<Option<rustls::pki_types::PrivateKeyDer<'static>>> {
	rustls_pemfile::private_key(reader)
		.map_err(|error| Error::Config(format!("could not read the private key: {error}")))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fingerprint_uses_openconnect_pin_format() {
		// A self-signed certificate is enough to exercise the SPKI hashing path.
		let pem = include_str!("../tests/fixtures/self-signed.pem");
		let der = rustls_pemfile_certs(&mut pem.as_bytes()).unwrap();
		let fingerprint = public_key_fingerprint(&der[0]).unwrap();
		assert!(fingerprint.starts_with("pin-sha256:"));
		// 32 raw bytes base64-encode to 44 characters including padding.
		assert_eq!(fingerprint.len(), "pin-sha256:".len() + 44);
	}
}
