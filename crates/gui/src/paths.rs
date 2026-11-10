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
mod tests {
    use super::*;

    #[test]
    fn paths_are_quoted_for_the_destination_shell() -> Result<()> {
        let path = Path::new("C:\\Research & Development\\it's (new).txt");
        assert_eq!(
            dropped(&[path], true, Some("cmd.exe"))?,
            "\"C:\\Research & Development\\it's (new).txt\" "
        );
        assert_eq!(
            dropped(
                &[path],
                true,
                Some("C:\\Program Files\\PowerShell\\pwsh.exe")
            )?,
            "'C:\\Research & Development\\it''s (new).txt' "
        );
        assert_eq!(
            dropped(&[Path::new("a'b"), Path::new("c d")], false, None)?,
            "'a'\\''b' 'c d' "
        );
        assert_eq!(
            dropped(&[Path::new("a'b")], true, Some("bash.exe"))?,
            "'a'\\''b' "
        );
        for path in ["%TEMP%\\file", "a!NAME!", "a\"b", "a\nb"] {
            assert!(dropped(&[Path::new(path)], true, Some("cmd.exe")).is_err());
        }
        assert!(dropped(&[path], true, Some("unknown.exe")).is_err());
        Ok(())
    }
}
