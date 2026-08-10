//! Small helpers over `roxmltree`, plus the error shape every GlobalProtect
//! endpoint shares.

use roxmltree::{Document, Node};

use crate::error::{Error, Result};

pub fn parse(body: &str) -> Result<Document<'_>> {
	Ok(Document::parse(body)?)
}

/// Text of the first descendant with this tag name.
pub fn text(node: Node<'_, '_>, tag: &str) -> Option<String> {
	node.descendants()
		.find(|child| child.has_tag_name(tag))
		.and_then(|child| child.text())
		.map(|value| value.trim().to_owned())
		.filter(|value| !value.is_empty())
}

/// Direct-child variant, for documents that repeat a tag at several depths.
pub fn child_text(node: Node<'_, '_>, tag: &str) -> Option<String> {
	node.children()
		.find(|child| child.has_tag_name(tag))
		.and_then(|child| child.text())
		.map(|value| value.trim().to_owned())
		.filter(|value| !value.is_empty())
}

pub fn find<'a, 'i>(node: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
	node.descendants().find(|child| child.has_tag_name(tag))
}

/// GlobalProtect signals failure inside a 200 response as often as it does with
/// an HTTP status, so every parsed body goes through this first.
pub fn check_for_error(body: &str) -> Result<()> {
	let trimmed = body.trim();
	if trimmed.is_empty() {
		return Err(Error::Parse("the server sent an empty response".into()));
	}
	// Some failures are a bare string rather than XML.
	if !trimmed.starts_with('<') {
		return Err(Error::Auth(trimmed.to_owned()));
	}
	let Ok(document) = Document::parse(trimmed) else {
		return Ok(());
	};
	let root = document.root_element();
	if root.has_tag_name("response") && root.attribute("status") == Some("error") {
		let message = text(root, "error").unwrap_or_else(|| "the server rejected the request".into());
		return Err(Error::Auth(message));
	}
	if let Some(message) = text(root, "msg") {
		if root.has_tag_name("error") {
			return Err(Error::Auth(message));
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_error_response_becomes_an_auth_error() {
		let body = r#"<response status="error"><error>Invalid username or password</error></response>"#;
		let error = check_for_error(body).unwrap_err();
		assert!(matches!(error, Error::Auth(message) if message == "Invalid username or password"));
	}

	#[test]
	fn a_success_response_passes() {
		assert!(check_for_error(r#"<response status="success"><foo/></response>"#).is_ok());
	}

	#[test]
	fn a_bare_string_failure_is_reported_verbatim() {
		let error = check_for_error("errors getting SSL/VPN config").unwrap_err();
		assert!(matches!(error, Error::Auth(message) if message == "errors getting SSL/VPN config"));
	}

	#[test]
	fn text_reads_nested_values() {
		let document = parse("<a><b><c> hello </c></b></a>").unwrap();
		assert_eq!(text(document.root_element(), "c").as_deref(), Some("hello"));
		assert_eq!(text(document.root_element(), "missing"), None);
	}
}
