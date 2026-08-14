use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = format!(
            "phosphorpulse-migrate-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_migrate(home: &Path, target: &Path, force: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_phosphorpulse"));
    command
        .arg("migrate")
        .env("HOME", home)
        .env("PPULSE_CONFIG_DIR", target)
        .env_remove("PPF_CONFIG_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if force {
        command.arg("--force");
    }
    command.output().expect("run phosphorpulse migrate")
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn write_source(home: &Path, settings: &str, shared: &str) -> PathBuf {
    let source = home.join(".claude/phosphorflux");
    fs::create_dir_all(source.join("pomodoro")).expect("create pomodoro source directory");
    fs::create_dir_all(source.join("lookups")).expect("create lookup source directory");
    fs::write(source.join("settings.json"), settings).expect("write source settings");
    fs::write(source.join("pomodoro/shared.json"), shared).expect("write source shared state");
    fs::write(source.join("pomodoro/shared.json.tmp-123"), "junk").expect("write temporary junk");
    fs::write(source.join("pomodoro/abc.json"), "historical").expect("write historical state");
    fs::write(source.join("lookups/cache.json"), "cache").expect("write lookup cache");
    source
}

// REQ-06 / S-07: migration is a whitelist copy with idempotence and partial failures.
#[test]
fn test_s07_migrate_semantics() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let target = temp.path().join("custom-target");
    write_source(&home, "source-settings-v1", "source-shared-v1");

    let first = run_migrate(&home, &target, false);
    let first_text = output_text(&first);
    assert!(first.status.success(), "first migration must exit 0: {first_text}");
    assert!(
        first_text.contains("copied")
            && first_text.contains("settings.json")
            && first_text.contains("pomodoro/shared.json"),
        "first migration must report both copied files: {first_text}"
    );
    assert_eq!(fs::read_to_string(target.join("settings.json")).expect("read copied settings"), "source-settings-v1");
    assert_eq!(fs::read_to_string(target.join("pomodoro/shared.json")).expect("read copied shared state"), "source-shared-v1");
    assert!(!target.join("pomodoro/shared.json.tmp-123").exists());
    assert!(!target.join("pomodoro/abc.json").exists());
    assert!(!target.join("lookups/cache.json").exists());

    let second = run_migrate(&home, &target, false);
    let second_text = output_text(&second);
    assert!(second.status.success(), "second migration must exit 0: {second_text}");
    assert!(
        second_text.contains("skipped")
            && second_text.contains("settings.json")
            && second_text.contains("pomodoro/shared.json"),
        "second migration must report both skipped files: {second_text}"
    );

    fs::write(target.join("settings.json"), "target-settings").expect("overwrite target settings");
    fs::write(target.join("pomodoro/shared.json"), "target-shared").expect("overwrite target shared state");
    let forced = run_migrate(&home, &target, true);
    let forced_text = output_text(&forced);
    assert!(forced.status.success(), "forced migration must exit 0: {forced_text}");
    assert_eq!(fs::read_to_string(target.join("settings.json")).expect("read forced settings"), "source-settings-v1");
    assert_eq!(fs::read_to_string(target.join("pomodoro/shared.json")).expect("read forced shared state"), "source-shared-v1");

    let absent_home = temp.path().join("absent-home");
    let absent = run_migrate(&absent_home, &temp.path().join("absent-target"), false);
    let absent_text = output_text(&absent);
    assert!(!absent.status.success(), "missing source must fail: {absent_text}");
    assert!(absent_text.contains("phosphorflux"), "missing-source error must be clear: {absent_text}");

    let unreadable_home = temp.path().join("unreadable-home");
    let unreadable_source = write_source(&unreadable_home, "blocked-settings", "readable-shared");
    let blocked = unreadable_source.join("settings.json");
    #[cfg(unix)]
    fs::set_permissions(&blocked, std::os::unix::fs::PermissionsExt::from_mode(0o000))
        .expect("make source settings unreadable");
    let unreadable_target = temp.path().join("unreadable-target");
    let unreadable = run_migrate(&unreadable_home, &unreadable_target, false);
    let unreadable_text = output_text(&unreadable);
    #[cfg(unix)]
    fs::set_permissions(&blocked, std::os::unix::fs::PermissionsExt::from_mode(0o644))
        .expect("restore source settings permissions");
    assert!(!unreadable.status.success(), "unreadable source must fail overall: {unreadable_text}");
    assert!(unreadable_text.contains("settings.json"), "unreadable file failure must be reported: {unreadable_text}");
    assert_eq!(fs::read_to_string(unreadable_target.join("pomodoro/shared.json")).expect("read sibling copied despite failure"), "readable-shared");
}
