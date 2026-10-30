//! Separate developer-only entry point. Never packaged in Coban-mod.app / Coban-mod.exe.
#[allow(dead_code)]
mod app;
slint::include_modules!();
fn main() {
    if let Err(e) = app::run_qa() {
        eprintln!("UI QA: {e:#}");
        std::process::exit(1);
    }
}
