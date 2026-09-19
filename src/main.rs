use phosphorpulse::{config, migrate, protocol, render, tui};
use std::io::IsTerminal;

fn main() {
    match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [command] if command == "render" => render_command(render::render),
        [command, flag] if command == "render" && flag == "--subagent" => {
            render_command(render::render_subagent)
        }
        [command] if command == "config" => config::config(),
        [command] if command == "usage-refresh" => {
            std::process::exit(phosphorpulse::usage::refresh_command())
        }
        [command, name, cwd] if command == "cmd-refresh" => {
            std::process::exit(phosphorpulse::cmd::refresh_command(name, cwd))
        }
        [command] if command == "migrate" => migrate_command(false),
        [command, flag] if command == "migrate" && flag == "--force" => migrate_command(true),
        [] => no_args(),
        _ => {}
    }
}

fn migrate_command(force: bool) {
    if migrate::migrate(force).is_err() {
        std::process::exit(1);
    }
}

fn render_command(render: fn(serde_json::Value, Vec<u8>)) {
    let (raw, raw_stdin) = match protocol::read_stdin_json() {
        Ok(input) => input,
        Err(reason) => {
            eprintln!("phosphorpulse: invalid stdin: {reason}");
            std::process::exit(1);
        }
    };
    render(raw, raw_stdin);
}

fn no_args() {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        match tui::run_tui() {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("phosphorpulse: {error}");
                std::process::exit(1);
            }
        }
    }
    println!("phosphorpulse requires an interactive terminal to open its TUI.");
    std::process::exit(1);
}
