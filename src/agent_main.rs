#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
fn main() {
    #[cfg(target_os = "macos")]
    if !std::env::args().any(|arg| arg == "--session-agent") {
        // A Login Item launch has no session flag. Keep the headless helper as
        // the stable input-permission owner and open its containing UI quietly.
        if let Err(error) = launch_menubar() {
            eprintln!("Coban-mod menubar: {error:#}");
        }
    }
    if let Err(error) = mk::service::serve() {
        eprintln!("Coban-mod Agent: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn launch_menubar() -> anyhow::Result<()> {
    use anyhow::Context;
    let current = std::env::current_exe()?;
    let Some(contents) = current.ancestors().nth(6) else {
        return Ok(());
    };
    if !current.ends_with("Library/LoginItems/MK Agent.app/Contents/MacOS/mk-agent") {
        return Ok(()); // Unbundled developer/test agent.
    }
    std::process::Command::new(contents.join("MacOS/Coban-mod"))
        .arg("--background")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Không mở được Coban-mod trên menubar")?;
    Ok(())
}
