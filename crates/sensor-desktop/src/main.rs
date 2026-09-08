#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod remote_input;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    if let Err(error) = ui::run() {
        rfd::MessageDialog::new()
            .set_title("SENSOR Remote Access")
            .set_description(format!(
                "SENSOR could not start. No listener was started.\n\n{error}"
            ))
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("SENSOR desktop currently requires Windows.");
    std::process::exit(1);
}
