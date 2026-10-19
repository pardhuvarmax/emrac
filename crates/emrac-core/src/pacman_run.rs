//! Runs a pacman transaction with its output shown in emrac's style (see
//! `pacman_ui`). pacman's standard output and error go to a pseudo-terminal, so it
//! draws its real progress bars; its input stays on the real terminal, so the
//! questions it asks are answered where they are shown, `sudo` asks for its
//! password on the terminal as always and keeps its per-terminal ticket, and
//! Ctrl-C reaches pacman as it does without emrac.

use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Command, ExitStatus, Stdio};

use crate::pacman_ui::{Renderer, Screen};
use crate::style::Style;

/// Whether this run is shown in emrac's style: a terminal that can draw it, input
/// from a terminal (so there is somebody to ask), a pacman command, and the setting.
pub fn wanted(st: &Style, program: &str, args: &[String]) -> bool {
    let pacman = (program == "sudo" && args.first().map(String::as_str) == Some("pacman"))
        || program == "pacman";
    pacman
        && st.fancy
        && !args.iter().any(|a| a == "--help" || a == "-h")
        && unsafe { libc::isatty(0) == 1 }
        && crate::Config::load().map_or(true, |c| c.clui.pacman != "native")
}

fn winsize() -> libc::winsize {
    let ws = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    for fd in [1, 2, 0] {
        let mut probe = ws;
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut probe) } == 0 && probe.ws_col > 0 {
            return probe;
        }
    }
    ws
}

extern "C" fn ignore(_: libc::c_int) {}

/// How a command that was shown ended.
#[derive(Debug, Clone, Copy)]
pub struct Shown {
    pub status: ExitStatus,
    /// It stopped at a question and printed nothing after it: the answer was no.
    pub declined: bool,
}

/// Runs the command and shows what it says in pacman's style. `Ok(None)`: a
/// pseudo-terminal could not be made, nothing ran, the caller should run it as
/// it always did.
pub fn run(program: &str, args: &[String], st: Style) -> std::io::Result<Option<Shown>> {
    run_screen(program, args, None, &mut Renderer::new(st))
}

/// Runs the command with its output on a pseudo-terminal, drawn by `screen`.
pub fn run_screen(
    program: &str,
    args: &[String],
    cwd: Option<&std::path::Path>,
    screen: &mut dyn Screen,
) -> std::io::Result<Option<Shown>> {
    let ws = winsize();
    let (mut m, mut s) = (-1, -1);
    if unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) } != 0 {
        return Ok(None);
    }
    let master = unsafe { OwnedFd::from_raw_fd(m) };
    let slave = unsafe { OwnedFd::from_raw_fd(s) };
    unsafe {
        libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let slave_err = slave.try_clone()?;

    // Ctrl-C goes to the program (it is in the same foreground group); emrac
    // stays to finish the screen. A handler, not "ignore", so the program
    // starts with the default again.
    let old = unsafe { libc::signal(libc::SIGINT, ignore as *const () as libc::sighandler_t) };
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::from(slave))
        .stderr(Stdio::from(slave_err));
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            unsafe { libc::signal(libc::SIGINT, old) };
            return Err(e);
        }
    };

    let mut stdout = std::io::stdout();
    let mut show = |bytes: Vec<u8>| -> bool {
        bytes.is_empty()
            || stdout
                .write_all(&bytes)
                .and_then(|()| stdout.flush())
                .is_ok()
    };
    let mut width = usize::from(ws.ws_col);
    let mut buf = [0u8; 8192];
    loop {
        let mut pfd = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pfd, 1, 100) };
        // the terminal may have been resized: the program is told, and the
        // live line is drawn again to fit
        let now = winsize();
        if usize::from(now.ws_col) != width {
            width = usize::from(now.ws_col);
            unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &now) };
            if !show(screen.resize(width)) {
                break;
            }
        }
        if ready > 0 {
            let n = unsafe { libc::read(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break; // EIO: everything that had the other end has closed it
            }
            if !show(screen.feed(&buf[..n as usize])) {
                break;
            }
        } else if ready == 0 {
            show(screen.flush_stale());
            // the program is gone and something else still holds the other end
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
        } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            break;
        }
        show(screen.tick());
    }
    show(screen.finish());
    let at_question = screen.left_at_a_question();
    let status = child.wait();
    unsafe { libc::signal(libc::SIGINT, old) };
    status.map(|status| {
        Some(Shown {
            status,
            declined: at_question && status.code() == Some(1),
        })
    })
}

/// For a test: the pseudo-terminal opens and the size is read.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_pacman_command_on_a_terminal_is_wanted() {
        let st = Style {
            fancy: false,
            ..Style::PLAIN
        };
        let args = vec!["pacman".to_string(), "-S".to_string(), "x".to_string()];
        assert!(!wanted(&st, "sudo", &args), "not a terminal");
        let fancy = Style {
            fancy: true,
            ..Style::PLAIN
        };
        assert!(!wanted(&fancy, "sudo", &["makepkg".to_string()]));
        assert!(!wanted(&fancy, "git", &args));
        assert!(!wanted(
            &fancy,
            "sudo",
            &["pacman".to_string(), "--help".to_string()]
        ));
    }

    #[test]
    fn the_size_is_never_zero() {
        let ws = winsize();
        assert!(ws.ws_col > 0 && ws.ws_row > 0);
    }

    #[test]
    fn a_command_with_a_terminal_for_output_is_shown_through_the_renderer() {
        // `printf` writes to the pseudo-terminal; the renderer passes it through
        let st = Style {
            fancy: true,
            ..Style::PLAIN
        };
        let status = run(
            "sh",
            &["-c".to_string(), "printf 'hello\\n'; exit 3".to_string()],
            st,
        )
        .unwrap()
        .unwrap();
        assert_eq!(status.status.code(), Some(3));
        assert!(!status.declined);
    }

    #[test]
    fn a_command_that_stops_at_a_question_with_a_failure_status_was_answered_no() {
        let st = Style {
            fancy: true,
            ..Style::PLAIN
        };
        let ask = |script: &str| {
            run("sh", &["-c".to_string(), script.to_string()], st)
                .unwrap()
                .unwrap()
        };
        let no = ask("printf ':: Proceed with installation? [Y/n] '; exit 1");
        assert!(no.declined && no.status.code() == Some(1));
        // an error after the question is a failure, not an answer
        let failed =
            ask("printf ':: Proceed with installation? [Y/n] '; printf 'error: failed\\n'; exit 1");
        assert!(!failed.declined);
        // success after a question is not a decline either
        assert!(!ask("printf ':: Proceed? [Y/n] '; exit 0").declined);
        // no question at all
        assert!(!ask("exit 1").declined);
    }
}
