//! Tiny OS helpers. `open_url` opens an https link in the user's default browser
//! (the version badge's "Open release"); argv-direct, no shell. Ported from the Tauri
//! backend's `open_url` command. Only https is honored, so it can't be coaxed into a
//! local file or a `file://`/`javascript:` scheme.

use std::process::Command;

pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("refusing to open a non-https URL".into());
    }
    #[cfg(target_os = "macos")]
    let (bin, args): (&str, Vec<&str>) = ("open", vec![url]);
    #[cfg(target_os = "linux")]
    let (bin, args): (&str, Vec<&str>) = ("xdg-open", vec![url]);
    #[cfg(target_os = "windows")]
    let (bin, args): (&str, Vec<&str>) = ("cmd", vec!["/C", "start", "", url]);
    Command::new(bin)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't open the browser: {e}"))
}
