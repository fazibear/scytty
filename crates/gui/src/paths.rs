use anyhow::{Context, Result, bail};
use std::path::Path;

pub fn dropped(paths: &[&Path], windows: bool, bin: Option<&str>) -> Result<String> {
    let default_bin = if windows {
        std::env::var("COMSPEC").ok()
    } else {
        None
    };
    let bin = bin.or(default_bin.as_deref());
    paths
        .iter()
        .map(|path| {
            quote(
                path.to_str()
                    .context("Dropped file path is not valid UTF-8")?,
                windows,
                bin,
            )
        })
        .collect::<Result<Vec<_>>>()
        .map(|paths| paths.join(" ") + " ")
}

fn quote(path: &str, windows: bool, bin: Option<&str>) -> Result<String> {
    if !windows {
        return Ok(format!("'{}'", path.replace('\'', "'\\''")));
    }
    let shell = bin
        .unwrap_or("cmd.exe")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match shell.as_str() {
        "cmd" | "cmd.exe" => {
            // cmd expands variables even inside quotes; refuse rather than paste another path/command.
            if path.contains(['%', '!', '"', '\r', '\n', '\0']) {
                bail!(
                    "Cannot safely paste this path into cmd.exe: it contains variable expansion or quote characters"
                );
            }
            Ok(format!("\"{path}\""))
        }
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => {
            Ok(format!("'{}'", path.replace('\'', "''")))
        }
        "sh" | "sh.exe" | "bash" | "bash.exe" | "zsh" | "zsh.exe" | "fish" | "fish.exe"
        | "dash" | "dash.exe" => quote(path, false, None),
        _ => bail!(
            "Cannot quote dropped paths for {shell}; use cmd.exe, PowerShell, or a POSIX shell"
        ),
    }
}

#[cfg(test)]
#[path = "../tests/unit/paths.rs"]
mod tests;
