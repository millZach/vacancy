//! `vacancy-index-reader --volume C: --pipe <32 hex digits>`: Vacancy's fast scan helper. Windows asks for admin to
//! start it; it reads the volume's file index, read-only, and sends folders, names, sizes and dates to the Vacancy
//! process that started it, over a pipe that process connects to. It never reads a file's contents, never writes,
//! moves or deletes anything, and takes nothing else from the app. See ADR 0007.
//!
//! Exit codes: 0 sent the index; 1 couldn't read the volume (the app was told why); 2 bad arguments; 3 the app
//! didn't connect, or something else did; 4 the pipe failed while sending.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(reader::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("vacancy-index-reader runs on Windows only.");
    std::process::exit(2);
}

#[cfg(windows)]
mod reader {
    use std::io::BufWriter;
    use std::time::Duration;

    use vacancy_index::windows::{
        Pipe, TokenAccess, VolumeDisk, harden_dll_search, is_local_drive, parent, parse_args,
    };
    use vacancy_index::wire::{Encoder, ErrorCode};

    /// The longest a read may take before the reader gives up on its own: a first read of a huge, slow drive is
    /// minutes, never this.
    const LONGEST: Duration = Duration::from_secs(30 * 60);

    pub fn run() -> i32 {
        harden_dll_search();
        let args: Vec<String> = std::env::args().skip(1).collect();
        let Some(args) = parse_args(&args) else {
            return 2;
        };
        if !is_local_drive(args.letter) {
            return 2;
        }
        // However it ends, it ends: nothing here is allowed to linger elevated.
        std::thread::spawn(|| {
            std::thread::sleep(LONGEST);
            std::process::exit(5);
        });
        let Ok((parent_pid, parent_sid)) = parent() else {
            return 3;
        };
        let Ok(pipe) = Pipe::create(&args.token, &parent_sid) else {
            return 3;
        };
        if pipe.accept(Duration::from_secs(60), parent_pid).is_err() {
            return 3;
        }
        let Ok(token) = pipe.hello_and_token(Duration::from_secs(10)) else {
            return 3;
        };
        let mut access = TokenAccess::new(token);
        let mut out = Encoder::new(BufWriter::with_capacity(1 << 20, &pipe));
        let code = match VolumeDisk::open(args.letter) {
            Err(_) => {
                let _ = out.error(ErrorCode::OpenVolume);
                1
            }
            // A pipe that fails stops the read through the errors it returns.
            Ok(disk) => match vacancy_index::serve(disk, &mut access, &mut out, &|| false) {
                Ok(Some(_)) => 0,
                Ok(None) => 4,
                Err(vacancy_index::IndexError::Io(_)) => 4,
                Err(_) => 1,
            },
        };
        // Wait for the app to read the last bytes before the pipe closes.
        let code = match out.into_inner().into_inner() {
            Ok(_) => code,
            Err(_) => 4,
        };
        pipe.wait_drained();
        code
    }
}
