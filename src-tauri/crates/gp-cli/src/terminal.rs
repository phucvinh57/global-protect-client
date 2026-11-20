//! Terminal-only prompts: secrets never come from arguments or redirected input.
use std::fs::OpenOptions;
use std::io::{IsTerminal, Read, Write};
use std::os::fd::AsRawFd;

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

pub fn prompt(label: &str, secret: bool) -> Result<String, String> {
    if !interactive() {
        return Err(format!("{label} requires an interactive terminal"));
    }
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| e.to_string())?;
    write!(tty, "{label}: ").map_err(|e| e.to_string())?;
    tty.flush().map_err(|e| e.to_string())?;
    let fd = tty.as_raw_fd();
    let mut original = std::mem::MaybeUninit::uninit();
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let original = unsafe { original.assume_init() };
    struct Restore(i32, libc::termios);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(self.0, libc::TCSANOW, &self.1);
            }
        }
    }
    let _restore = Restore(fd, original);
    let mut mode = original;
    mode.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG);
    mode.c_cc[libc::VMIN] = 1;
    mode.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &mode) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut result = Vec::new();
    loop {
        let mut byte = [0];
        tty.read_exact(&mut byte).map_err(|e| e.to_string())?;
        match byte[0] {
            b'\n' | b'\r' => {
                writeln!(tty).map_err(|e| e.to_string())?;
                break;
            }
            3 | 4 => {
                let _ = writeln!(tty);
                return Err("Prompt cancelled".into());
            }
            8 | 127 => {
                if result.pop().is_some() && !secret {
                    let _ = tty.write_all(b"\x08 \x08");
                }
            }
            b if b >= 32 => {
                result.push(b);
                if !secret {
                    tty.write_all(&byte).map_err(|e| e.to_string())?;
                }
            }
            _ => {}
        }
    }
    let text = String::from_utf8(result).map_err(|_| "Input must be UTF-8")?;
    if text.trim().is_empty() {
        return Err(format!("{label} is required"));
    }
    Ok(text)
}

pub fn confirm(label: &str) -> Result<bool, String> {
    if !interactive() {
        return Err(format!("{label} requires an interactive terminal"));
    }
    // Explicit yes only. An empty answer means no.
    match prompt(&format!("{label} [yes/no]"), false) {
        Ok(value) => Ok(value.eq_ignore_ascii_case("yes")),
        Err(e) if e.ends_with(" is required") => Ok(false),
        Err(e) => Err(e),
    }
}
