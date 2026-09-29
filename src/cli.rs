// SPDX-License-Identifier: GPL-3.0-or-later

//! Parsing de `argv` (ADR-0040): superfície fechada de cinco formas --
//! `porecatu`, `porecatu <diretório>`, `--config <arquivo>`, `--help`/
//! `-h`, `--version`/`-V`. Laço à mão sobre `OsString`, sem crate de CLI
//! (motivo escrito no ADR: cinco formas não justificam a dependência).
//!
//! **O texto impresso aqui é inglês fixo, fora do catálogo** (ADR-0056 §3,
//! que revê o ADR-0040 §1): `argv` é lido antes de a config existir, e
//! traduzir o `--help` exigiria resolver config e idioma antes de parsear o
//! argumento que diz qual config ler. O binário não depende de
//! `porecatu-locale`, e este texto não conta como interface para a métrica
//! de "uma frase no código".
//!
//! `parse` é pura -- recebe os argumentos, devolve um resultado -- pelo
//! mesmo motivo de `porecatu_config::path::resolve`: testável sem
//! processo. Mora no binário, não em `porecatu-ui`: `argv` é do
//! processo, e o binário é a única camada que pode lê-lo sem furar a
//! regra de dependência do CLAUDE.md.

use std::ffi::OsString;
use std::path::PathBuf;

/// Resultado do parse -- o que `main` faz com cada variante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    Help,
    Version,
    Run {
        config: Option<PathBuf>,
        directory: Option<PathBuf>,
    },
}

/// Fonte única das formas aceitas -- `help_text` e o teste que a compara
/// ao parser leem daqui, para as duas nunca divergirem em silêncio.
const FORMS: &[(&str, &str)] = &[
    ("porecatu", "Restores the last saved session"),
    (
        "porecatu <directory>",
        "New session in that directory; does not restore, does not overwrite",
    ),
    (
        "porecatu --config <file>",
        "Uses that config file, overriding PORECATU_CONFIG and the platform path",
    ),
    ("porecatu --help / -h", "Prints the forms above and exits"),
    (
        "porecatu --version / -V",
        "Prints name, version and license, and exits",
    ),
];

pub fn help_text() -> String {
    let mut text = String::from("Usage:\n");
    for (form, effect) in FORMS {
        text.push_str(&format!("  {form}\n      {effect}\n"));
    }
    text
}

pub fn version_text() -> String {
    format!(
        "{} {}\n{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_LICENSE")
    )
}

/// Núcleo puro do parse (ADR-0040 §3): laço sobre `args_os` (nunca
/// `args()` -- caminho no Windows pode não ser UTF-8, e caminho nunca
/// passa por `String`). `--help`/`--version` encerram o parse na hora em
/// que aparecem, mesmo com outros argumentos antes ou depois.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    let mut config: Option<PathBuf> = None;
    let mut directory: Option<PathBuf> = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help") | Some("-h") => return Ok(Cli::Help),
            Some("--version") | Some("-V") => return Ok(Cli::Version),
            Some("--config") => {
                let value = args
                    .next()
                    .ok_or_else(|| "--config requires an argument: <file>".to_string())?;
                config = Some(PathBuf::from(value));
            }
            // Qualquer outra coisa começando com `-` é flag desconhecida --
            // nunca cai no ramo de caminho posicional, que só aceita o
            // argumento cru. Um caminho não-UTF-8 (`to_str` devolve
            // `None`) nunca começa com um `-` ASCII reconhecível aqui, e
            // cai direto no último ramo, como qualquer outro caminho.
            Some(unknown) if unknown.starts_with('-') && unknown != "-" => {
                return Err(format!("unknown argument: {unknown}"));
            }
            _ => {
                if directory.is_some() {
                    return Err("more than one positional path".to_string());
                }
                directory = Some(PathBuf::from(arg));
            }
        }
    }

    Ok(Cli::Run { config, directory })
}

/// RF-3.12/ADR-0040 §2: validação do caminho posicional, separada do
/// parse -- toca disco, então não é pura, e roda só depois que o parse
/// (que é) já decidiu que há um caminho para validar. Caminho inexistente
/// ou que é arquivo em vez de diretório é erro visível, nunca cai no home
/// em silêncio (isso é o RF-3.10, para um `cwd` **gravado** que sumiu --
/// situação diferente de um caminho que o usuário acabou de digitar).
pub fn validate_directory(path: &std::path::Path) -> Result<(), String> {
    let metadata =
        std::fs::metadata(path).map_err(|_| format!("directory not found: {}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_restores_the_last_session() {
        assert_eq!(
            parse(args(&[])).unwrap(),
            Cli::Run {
                config: None,
                directory: None,
            }
        );
    }

    #[test]
    fn a_bare_path_is_the_positional_directory() {
        assert_eq!(
            parse(args(&["/tmp/projeto"])).unwrap(),
            Cli::Run {
                config: None,
                directory: Some(PathBuf::from("/tmp/projeto")),
            }
        );
    }

    #[test]
    fn config_flag_sets_the_config_path() {
        assert_eq!(
            parse(args(&["--config", "/etc/porecatu.toml"])).unwrap(),
            Cli::Run {
                config: Some(PathBuf::from("/etc/porecatu.toml")),
                directory: None,
            }
        );
    }

    #[test]
    fn config_and_positional_combine_in_either_order() {
        let expected = Cli::Run {
            config: Some(PathBuf::from("/etc/porecatu.toml")),
            directory: Some(PathBuf::from("/tmp/projeto")),
        };
        assert_eq!(
            parse(args(&["--config", "/etc/porecatu.toml", "/tmp/projeto"])).unwrap(),
            expected
        );
        assert_eq!(
            parse(args(&["/tmp/projeto", "--config", "/etc/porecatu.toml"])).unwrap(),
            expected
        );
    }

    #[test]
    fn help_forms_are_recognized() {
        assert_eq!(parse(args(&["--help"])).unwrap(), Cli::Help);
        assert_eq!(parse(args(&["-h"])).unwrap(), Cli::Help);
        // Ganha mesmo com outro argumento antes.
        assert_eq!(parse(args(&["/tmp/projeto", "--help"])).unwrap(), Cli::Help);
    }

    #[test]
    fn version_forms_are_recognized() {
        assert_eq!(parse(args(&["--version"])).unwrap(), Cli::Version);
        assert_eq!(parse(args(&["-V"])).unwrap(), Cli::Version);
    }

    #[test]
    fn unknown_flag_is_an_error() {
        assert!(parse(args(&["--session"])).is_err());
        assert!(parse(args(&["-e"])).is_err());
    }

    #[test]
    fn config_without_a_value_is_an_error() {
        assert!(parse(args(&["--config"])).is_err());
    }

    #[test]
    fn two_positionals_is_an_error() {
        assert!(parse(args(&["/tmp/a", "/tmp/b"])).is_err());
    }

    /// `args_os`/`OsString` nunca quebram num argumento que não é UTF-8
    /// válido -- ele cai no ramo de caminho posicional como qualquer
    /// outro, sem passar por `String`.
    #[test]
    fn non_utf8_argument_does_not_break_the_parse() {
        let invalid = non_utf8_os_string();
        let result = parse(vec![invalid.clone()]).unwrap();
        assert_eq!(
            result,
            Cli::Run {
                config: None,
                directory: Some(PathBuf::from(invalid)),
            }
        );
    }

    #[cfg(windows)]
    fn non_utf8_os_string() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        // 0xD800 é um surrogate solto -- inválido isoladamente em UTF-16,
        // então nunca decodifica para UTF-8 válido.
        OsString::from_wide(&[0xD800])
    }

    #[cfg(unix)]
    fn non_utf8_os_string() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        // 0xFF sozinho nunca é um byte inicial válido de UTF-8.
        OsString::from_vec(vec![0xFF])
    }

    #[test]
    fn help_text_lists_every_accepted_form() {
        let text = help_text();
        for (form, _) in FORMS {
            assert!(text.contains(form), "form missing from the help: {form}");
        }
    }

    /// ADR-0056 §3: a linha de comando é inglês fixo, sem catálogo.
    #[test]
    fn help_text_is_english() {
        let text = help_text();
        assert!(text.starts_with("Usage:\n"));
        assert!(text.contains("Restores the last saved session"));
        assert!(!text.contains("Uso:"));
        assert!(text.is_ascii(), "the help has no accented character");
    }

    #[test]
    fn argument_errors_are_english() {
        assert_eq!(
            parse(args(&["--session"])).unwrap_err(),
            "unknown argument: --session"
        );
        assert_eq!(
            parse(args(&["--config"])).unwrap_err(),
            "--config requires an argument: <file>"
        );
        assert_eq!(
            parse(args(&["/tmp/a", "/tmp/b"])).unwrap_err(),
            "more than one positional path"
        );
    }

    #[test]
    fn directory_errors_are_english() {
        let missing = std::env::temp_dir().join("porecatu-cli-no-such-dir-xyz");
        let err = validate_directory(&missing).unwrap_err();
        assert!(err.starts_with("directory not found: "), "{err}");
        // Um arquivo existente no lugar do diretório.
        let file = std::env::current_exe().unwrap();
        let err = validate_directory(&file).unwrap_err();
        assert!(err.starts_with("not a directory: "), "{err}");
    }
}
