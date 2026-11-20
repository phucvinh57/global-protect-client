//! CLI startup: leave sudo's terminal session before any worker threads exist.
//! The original JSON-lines pipes stay open; EOF still cancels and cleans up.
pub fn detach_if_requested() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() {
        return Ok(());
    }
    if args.len() != 1 || args[0] != "--detach" {
        return Err("Usage: gp-helper [--detach]".into());
    }
    // The parent exits so sudo's monitor exits too. A monitor left attached to
    // the terminal could otherwise forward SIGHUP to a surviving helper.
    match unsafe { libc::fork() } {
        -1 => Err(std::io::Error::last_os_error().to_string()),
        0 => {
            if unsafe { libc::setsid() } < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
        _ => unsafe { libc::_exit(0) },
    }
}
