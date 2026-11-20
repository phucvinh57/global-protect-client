#[cfg(not(target_os = "linux"))]
compile_error!("The standalone gp-client CLI currently supports Linux only");

mod controller;
mod terminal;

use clap::{Args, Parser, Subcommand};
use gp_store::{
    credentials,
    settings::{self, Profile},
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(
    name = "gp-client",
    about = "Standalone GlobalProtect VPN client",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Manage profiles shared with the desktop application.
    Profile {
        #[command(subcommand)]
        command: Box<Profiles>,
    },
    /// Connect and leave the tunnel running in the background.
    Connect {
        profile: String,
    },
    Status,
    /// Wait until the CLI tunnel has cleaned up routing and DNS.
    Disconnect,
    #[command(name = "__controller", hide = true)]
    Controller,
}

#[derive(Subcommand)]
enum Profiles {
    Add {
        #[arg(id = "initial_name")]
        name: Option<String>,
        #[command(flatten)]
        options: ProfileOptions,
    },
    List,
    Show {
        profile: String,
    },
    Edit {
        profile: String,
        #[command(flatten)]
        options: ProfileOptions,
    },
    Delete {
        profile: String,
    },
}

#[derive(Args, Default)]
struct ProfileOptions {
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    portal: Option<String>,
    #[arg(long)]
    username: Option<String>,
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    remember: Option<bool>,
    /// Use an empty value to clear an optional field.
    #[arg(long)]
    cafile: Option<String>,
    #[arg(long)]
    client_cert: Option<String>,
    #[arg(long)]
    client_key: Option<String>,
    #[arg(long)]
    trusted_fingerprint: Option<String>,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long, value_parser = ["", "linux", "windows", "mac", "Linux", "Windows", "Mac"])]
    client_os: Option<String>,
    #[arg(long)]
    script: Option<String>,
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    hip: Option<bool>,
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    requires_otp: Option<bool>,
}

impl ProfileOptions {
    fn apply(self, profile: &mut Profile) {
        if let Some(v) = self.name {
            profile.name = v;
        }
        if let Some(v) = self.portal {
            profile.portal = v;
        }
        if let Some(v) = self.username {
            profile.username = v;
        }
        if let Some(v) = self.remember {
            profile.remember = v;
        }
        if let Some(v) = self.hip {
            profile.hip = v;
        }
        if let Some(v) = self.requires_otp {
            profile.requires_otp = v;
        }
        macro_rules! optional { ($($f:ident),*) => { $(if let Some(v) = self.$f { profile.$f = (!v.is_empty()).then_some(v); })* }; }
        optional!(
            cafile,
            client_cert,
            client_key,
            trusted_fingerprint,
            gateway,
            client_os,
            script
        );
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("gp-client: {error}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Action::Controller => controller::run().await,
        Action::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&controller::request("status").await?).unwrap()
            );
            Ok(())
        }
        Action::Disconnect => {
            controller::request("disconnect").await?;
            println!("Disconnected");
            Ok(())
        }
        action => {
            let pool = settings::open(&settings::database_path()?).await?;
            match action {
                Action::Connect { profile } => {
                    controller::ensure_available()?;
                    let mut profile = settings::resolve(&pool, &profile).await?;
                    let (password, otp) = credentials_for(&mut profile)?;
                    settings::save(&pool, &profile).await?;
                    pool.close().await;
                    connect(profile, password, otp)
                }
                Action::Profile { command } => {
                    match *command {
                        Profiles::List => {
                            for p in settings::load(&pool).await? {
                                println!("{}\t{}\t{}\t{}", p.id, p.name, p.portal, p.username);
                            }
                        }
                        Profiles::Show { profile } => println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &settings::resolve(&pool, &profile).await?
                            )
                            .unwrap()
                        ),
                        Profiles::Delete { profile } => {
                            let p = settings::resolve(&pool, &profile).await?;
                            if credentials::is_available() {
                                credentials::delete(&p.portal, &p.username)?;
                            }
                            settings::delete(&pool, &p.id).await?;
                        }
                        Profiles::Add { name, options } => {
                            let mut p = Profile {
                                id: settings::new_id(),
                                name: name.unwrap_or_default(),
                                ..Default::default()
                            };
                            options.apply(&mut p);
                            complete_profile(&mut p)?;
                            if p.remember
                                && !terminal::confirm(
                                    "Enable remembering passwords for this profile?",
                                )?
                            {
                                p.remember = false;
                            }
                            settings::save(&pool, &p).await?;
                            println!("{}", p.id);
                        }
                        Profiles::Edit { profile, options } => {
                            let old = settings::resolve(&pool, &profile).await?;
                            let mut p = old.clone();
                            options.apply(&mut p);
                            complete_profile(&mut p)?;
                            if p.remember
                                && !old.remember
                                && !terminal::confirm(
                                    "Enable remembering passwords for this profile?",
                                )?
                            {
                                p.remember = false;
                            }
                            settings::save(&pool, &p).await?;
                            if credentials::is_available()
                                && (!p.remember
                                    || old.portal != p.portal
                                    || old.username != p.username)
                            {
                                credentials::delete(&old.portal, &old.username)?;
                            }
                        }
                    }
                    pool.close().await;
                    Ok(())
                }
                _ => unreachable!(),
            }
        }
    }
}

fn complete_profile(p: &mut Profile) -> Result<(), String> {
    if p.portal.trim().is_empty() {
        p.portal = terminal::prompt("Portal", false)?;
    }
    if p.username.trim().is_empty() {
        p.username = terminal::prompt("Username", false)?;
    }
    if p.name.trim().is_empty() {
        p.name = settings::default_name(&p.portal);
    }
    Ok(())
}

fn credentials_for(p: &mut Profile) -> Result<(String, String), String> {
    let saved = p
        .remember
        .then(|| credentials::get(&p.portal, &p.username))
        .flatten();
    let password = match saved {
        Some(v) => v,
        None => {
            let password = terminal::prompt("Password", true)?;
            if credentials::is_available()
                && terminal::confirm("Remember this password in the system keyring?")?
            {
                credentials::set(&p.portal, &p.username, &password)?;
                p.remember = true;
            }
            password
        }
    };
    let otp = if p.requires_otp {
        terminal::prompt("One-time passcode", true)?
    } else {
        String::new()
    };
    Ok((password, otp))
}

fn send(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, value).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

fn connect(profile: Profile, password: String, otp: String) -> Result<(), String> {
    controller::ensure_available()?;
    let helper = controller::helper_path()?;
    // Authenticate on this terminal, before the controller loses its session.
    if unsafe { libc::geteuid() } != 0 {
        let status = Command::new("sudo")
            .arg(if terminal::interactive() { "-v" } else { "-n" })
            .args(if terminal::interactive() {
                vec![]
            } else {
                vec!["-v"]
            })
            .status()
            .map_err(|e| format!("Could not authenticate with sudo: {e}"))?;
        if !status.success() {
            return Err("sudo authentication failed; use an interactive terminal".into());
        }
    }
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("__controller")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().ok_or("Controller stdin missing")?;
    let mut output = BufReader::new(child.stdout.take().ok_or("Controller stdout missing")?);
    let mut request = json!({"cmd":"connect", "portal":profile.portal, "username":profile.username, "password":password, "otp":otp,
        "cafile":profile.cafile, "client_cert":profile.client_cert, "client_key":profile.client_key,
        "trusted_fingerprint":profile.trusted_fingerprint, "gateway":profile.gateway, "client_os":profile.client_os,
        "script":profile.script, "hip":profile.hip});
    send(
        &mut input,
        &json!({"helper":helper, "profile_id":profile.id, "profile_name":profile.name, "request":request}),
    )?;
    request.take();
    let result = (|| loop {
        let mut line = String::new();
        if output.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("Background controller exited before connection succeeded".into());
        }
        let event: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if event["ev"] == "connected" {
            println!(
                "Connected: {} ({})",
                profile.name,
                event["ifname"].as_str().unwrap_or("tunnel")
            );
            return Ok(());
        }
        if event["ev"] == "error" {
            return Err(event["msg"]
                .as_str()
                .unwrap_or("Connection failed")
                .to_owned());
        }
        if let Some(answer) = answer_event(&event)? {
            send(&mut input, &answer)?;
        }
    })();
    // EOF cancels authentication. Once connected it only detaches this terminal.
    drop(input);
    drop(output);
    if result.is_err() {
        let _ = child.wait();
    }
    result
}

fn answer_event(event: &Value) -> Result<Option<Value>, String> {
    match event["ev"].as_str().unwrap_or("") {
        "cert_untrusted" => {
            eprintln!(
                "{}\nFingerprint: {}",
                event["details"].as_str().unwrap_or(""),
                event["fingerprint"].as_str().unwrap_or("")
            );
            if !terminal::confirm("Trust this certificate for this session?")? {
                return Err("Certificate rejected".into());
            }
            Ok(Some(json!({"cmd":"trust_cert"})))
        }
        "mfa_challenge" => {
            eprintln!("{}", event["message"].as_str().unwrap_or("MFA challenge"));
            Ok(Some(
                json!({"cmd":"submit_otp", "otp":terminal::prompt("One-time passcode", true)?}),
            ))
        }
        "gateways" if event["selecting"] == true => {
            let list = event["list"]
                .as_array()
                .ok_or("Helper returned invalid gateway list")?;
            for (index, gateway) in list.iter().enumerate() {
                eprintln!(
                    "{}: {} ({})",
                    index + 1,
                    gateway["name"].as_str().unwrap_or(""),
                    gateway["address"].as_str().unwrap_or("")
                );
            }
            let index: usize = terminal::prompt("Gateway number", false)?
                .parse()
                .map_err(|_| "Invalid gateway number")?;
            let gateway = index
                .checked_sub(1)
                .and_then(|i| list.get(i))
                .ok_or("Invalid gateway number")?;
            Ok(Some(
                json!({"cmd":"select_gateway", "address":gateway["address"]}),
            ))
        }
        _ => Ok(None),
    }
}
