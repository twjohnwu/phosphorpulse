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
        [command] if command == "render" => render::render(),
        [command, flag] if command == "render" && flag == "--subagent" => render::render_subagent(),
        [command] if command == "config" => config::config(),
        [command] if command == "migrate" => migrate::migrate(),
        [] => no_args(),
        _ => {}
    }
}

fn no_args() {}
