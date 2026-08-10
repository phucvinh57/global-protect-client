//! Exercises the login against a real portal without root, a tunnel or the GUI.
//!
//! ```text
//! cargo run -p gp-auth --example probe -- https://vpn.example.com alice
//! ```
//!
//! Prints every request and the exact server reply, including the
//! `x-private-pan-globalprotect` header that explains an HTTP 512.

use std::{
	io::{self, Write},
	sync::Arc,
};

use gp_auth::{Answer, AuthRequest, ClientOs, Question, TlsOptions};

fn main() {
	let mut arguments = std::env::args().skip(1);
	let (Some(server), Some(username)) = (arguments.next(), arguments.next()) else {
		eprintln!("usage: probe <portal-url> <username> [linux|windows|mac] [gateway]");
		std::process::exit(2);
	};
	let client_os = ClientOs::parse(&arguments.next().unwrap_or_default());
	let gateway = arguments.next();

	let password = prompt("Password: ");
	let otp = prompt("OTP (blank if none): ");

	let request = AuthRequest {
		server,
		username,
		password,
		otp,
		gateway,
		client_os,
		tls: TlsOptions::default(),
	};

	let result = gp_auth::authenticate(
		request,
		Arc::new(|prompt| {
			eprintln!("\nUntrusted certificate {}\n{}", prompt.fingerprint, prompt.details);
			ask("Trust it? [y/N]: ").trim().eq_ignore_ascii_case("y")
		}),
		Arc::new(|message| eprintln!("  {message}")),
		Arc::new(|question| match question {
			Question::Challenge(challenge) => {
				eprintln!("\n{}", challenge.message);
				Answer::Otp(ask("Code: "))
			}
			Question::Gateway(gateways) => {
				eprintln!("\nGateways:");
				for (index, gateway) in gateways.iter().enumerate() {
					eprintln!("  [{index}] {} ({})", gateway.name, gateway.address);
				}
				let choice: usize = ask("Choose: ").trim().parse().unwrap_or(0);
				match gateways.get(choice) {
					Some(gateway) => Answer::Gateway(gateway.address.clone()),
					None => Answer::Cancelled,
				}
			}
		}),
	);

	match result {
		Ok(session) => {
			println!("\nAuthenticated against {} ({})", session.gateway_name, session.gateway_url);
			// The cookie is a bearer credential, so only its shape is printed.
			println!(
				"Cookie fields: {}",
				session
					.cookie
					.split('&')
					.filter_map(|pair| pair.split('=').next())
					.collect::<Vec<_>>()
					.join(", ")
			);
		}
		Err(error) => {
			eprintln!("\nFailed: {error}");
			std::process::exit(1);
		}
	}
}

fn prompt(label: &str) -> String {
	ask(label)
}

fn ask(label: &str) -> String {
	eprint!("{label}");
	let _ = io::stderr().flush();
	let mut line = String::new();
	let _ = io::stdin().read_line(&mut line);
	line.trim_end_matches(['\n', '\r']).to_owned()
}
