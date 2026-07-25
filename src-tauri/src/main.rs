// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop entrypoint — **temporarily non-building** pending #184.
//!
//! Domain + operator CLI live in workspace crates (`pageseeds-core`,
//! `pageseeds-cli`). This binary will call into core once the shell is rewired.

fn main() {
    // lib.rs emits compile_error! until #184; this body is unreachable in CI.
    pageseeds_lib::run();
}
