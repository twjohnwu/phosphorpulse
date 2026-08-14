mod atomic_write;
mod clock;
mod config;
mod jsx;
mod migrate;
mod protocol;
mod render;
mod segments;

fn main() {
    match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [command] if command == "render" => render_command(render::render),
        [command, flag] if command == "render" && flag == "--subagent" => {
            render_command(render::render_subagent)
        }
        [command] if command == "config" => config::config(),
        [command] if command == "migrate" => migrate::migrate(),
        [] => no_args(),
        _ => {}
    }
}

fn render_command(render: fn()) {
    if let Err(reason) = protocol::read_stdin_json() {
        eprintln!("phosphorpulse: invalid stdin: {reason}");
        std::process::exit(1);
    }
    render();
}

fn no_args() {
    println!(
        "The phosphorpulse TUI is not implemented; edit settings.json by hand or use the phosphorflux TUI, then migrate."
    );
    std::process::exit(1);
}
