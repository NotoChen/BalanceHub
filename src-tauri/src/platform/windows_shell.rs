use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

pub(crate) fn encoded_powershell(script: &str) -> String {
    STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

/// Quote a native Windows argv item, including trailing backslashes.
fn quote_argument(value: &str) -> String {
    let mut quoted = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        quoted.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        quoted.push(ch);
        slashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', slashes * 2));
    quoted.push('"');
    quoted
}

/// Build a shell command for a native executable. Ordinary paths take the
/// direct route; paths containing shell expansion characters use an encoded
/// PowerShell bridge with literal FileName/Arguments and inherited pipe handles.
pub(crate) fn native_command(program: &Path, arguments: &[String]) -> String {
    let program = super::paths::compatible_path(program)
        .to_string_lossy()
        .into_owned();
    let quoted_arguments = arguments
        .iter()
        .map(|value| {
            if !value.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'=')
                })
            {
                value.clone()
            } else {
                quote_argument(value)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let needs_literal_bridge = std::iter::once(program.as_str())
        .chain(arguments.iter().map(String::as_str))
        .any(|value| value.contains(['%', '!', '"', '\r', '\n']));
    if !needs_literal_bridge {
        return format!("{} {quoted_arguments}", quote_argument(&program));
    }
    let literal = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let script = format!(
        "try {{ $BH_Start = New-Object System.Diagnostics.ProcessStartInfo; $BH_Start.FileName = {}; $BH_Start.Arguments = {}; $BH_Start.UseShellExecute = $false; $BH_Start.CreateNoWindow = $true; $BH_Start.RedirectStandardInput = $false; $BH_Child = [System.Diagnostics.Process]::Start($BH_Start); $BH_Child.WaitForExit(); $BH_Exit = $BH_Child.ExitCode; $BH_Child.Dispose(); exit $BH_Exit }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}",
        literal(&program), literal(&quoted_arguments),
    );
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        encoded_powershell(&script)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_argv_quotes_preserve_empty_arguments_and_trailing_slashes() {
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument(r"C:\"), "\"C:\\\\\"");
        assert_eq!(quote_argument("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn shell_expansion_characters_are_encoded_as_literal_process_parameters() {
        let command = native_command(
            Path::new(r"C:\100%PATH% !\helper.exe"),
            &["--root".into(), "C:\\用户's\\data".into()],
        );
        assert!(command.starts_with("powershell.exe "));
        assert!(!command.contains("%PATH%"));
        let bytes = STANDARD
            .decode(command.split_whitespace().last().unwrap())
            .unwrap();
        let text = String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(text.contains(r"$BH_Start.FileName = 'C:\100%PATH% !\helper.exe'"));
        assert!(text.contains("用户''s"));
        assert!(text.contains("$BH_Start.RedirectStandardInput = $false"));
    }
}
