// SPDX-License-Identifier: GPL-3.0-or-later

// Sem console junto da janela no Windows -- sem efeito nos outros alvos
// (a diretiva é ignorada fora de `windows`), então não precisa de `cfg`.
#![windows_subsystem = "windows"]

mod cli;

/// ADR-0040: `--help`/`--version` imprimem e saem sem abrir janela;
/// argumento inválido (flag desconhecida, `--config` sem valor, dois
/// posicionais, ou caminho posicional inexistente/que não é diretório)
/// vira mensagem em `stderr` e saída com código diferente de zero, também
/// sem abrir janela -- o mesmo princípio nas duas formas de erro.
fn main() {
    let args = std::env::args_os().skip(1);
    match cli::parse(args) {
        Ok(cli::Cli::Help) => print!("{}", cli::help_text()),
        Ok(cli::Cli::Version) => println!("{}", cli::version_text()),
        Ok(cli::Cli::Run { config, directory }) => {
            if let Some(dir) = &directory
                && let Err(err) = cli::validate_directory(dir)
            {
                eprintln!("porecatu: {err}");
                std::process::exit(2);
            }
            porecatu_ui::run(config, directory);
        }
        Err(err) => {
            eprintln!("porecatu: {err}");
            std::process::exit(2);
        }
    }
}
