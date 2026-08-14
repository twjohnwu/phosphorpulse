use std::{
    fs, io,
    path::{Path, PathBuf},
};

const MIGRATION_FILES: [&str; 2] = ["settings.json", "pomodoro/shared.json"];

/// Copies the two persistent files from phosphorflux into phosphorpulse.
///
/// Each file is deliberately handled independently so a bad source file does
/// not prevent an otherwise valid sibling from being migrated.
pub fn migrate(force: bool) -> Result<(), ()> {
    let source_dir = home_dir().join(".claude/phosphorflux");
    if let Err(error) = validate_source_dir(&source_dir) {
        println!(
            "migration failed: cannot access phosphorflux source directory {}: {error}",
            source_dir.display(),
        );
        return Err(());
    }

    let target_dir = target_dir();
    let mut failed = false;

    for relative_path in MIGRATION_FILES {
        let source = source_dir.join(relative_path);
        let target = target_dir.join(relative_path);

        if !force && path_exists(&target) {
            println!("skipped {relative_path}: target already exists");
            continue;
        }

        match copy_file(&source, &target) {
            Ok(()) => println!("copied {relative_path}"),
            Err(error) => {
                println!("failed {relative_path}: {error}");
                failed = true;
            }
        }
    }

    if failed {
        Err(())
    } else {
        Ok(())
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn target_dir() -> PathBuf {
    std::env::var_os("PPULSE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude/phosphorpulse"))
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn validate_source_dir(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "source path is not a directory",
        ))
    }
}

fn copy_file(source: &Path, target: &Path) -> std::io::Result<()> {
    // Open first to surface source read failures before creating a destination.
    // `fs::copy` follows source symlinks and writes a regular destination file.
    fs::File::open(source)?;
    let parent = target.parent().expect("migration targets have parents");
    fs::create_dir_all(parent)?;
    fs::copy(source, target)?;
    Ok(())
}
