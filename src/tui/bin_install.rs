//! Shared global-binary detection and installation helpers for the TUI.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "phosphorpulse.exe"
    } else {
        "phosphorpulse"
    }
}

pub fn local_bin_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".local/bin")
            .join(executable_name())
    })
}

pub fn bin_on_path() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(executable_name()))
            .find(|candidate| candidate.is_file())
    })
}

pub fn detected_bin() -> Option<PathBuf> {
    bin_on_path().or_else(|| local_bin_path().filter(|candidate| candidate.is_file()))
}

/// Atomically replaces the global binary with the current executable.
pub fn install_global_bin(destination: &Path) -> io::Result<()> {
    let source = std::env::current_exe()?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("global bin has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("phosphorpulse");
    let temporary = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        io::copy(&mut input, &mut output)?;
        output.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
