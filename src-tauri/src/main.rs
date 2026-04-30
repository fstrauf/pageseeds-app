// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        if let Err(e) = pageseeds_lib::run_cli(args) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    pageseeds_lib::run();
}
