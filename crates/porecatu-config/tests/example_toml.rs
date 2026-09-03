// SPDX-License-Identifier: GPL-3.0-or-later

//! Auditoria da etapa 1 da F4 (ADR-0003, ADR-0030): `docs/config/porecatu.example.toml`
//! e `Config::default()` têm de ser exatamente a mesma config, chave por
//! chave. É o que prova que o arquivo de exemplo e os defaults do código
//! não divergiram.

use porecatu_config::{Config, LoadResult};

fn example_toml_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/config/porecatu.example.toml")
}

#[test]
fn example_toml_matches_default_config() {
    let path = example_toml_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("não foi possível ler {}: {err}", path.display()));

    let (config, unknown_keys) = porecatu_config::parse(&text)
        .unwrap_or_else(|err| panic!("porecatu.example.toml inválido: {err}"));

    assert!(
        unknown_keys.is_empty(),
        "chaves desconhecidas no arquivo de exemplo: {unknown_keys:?}"
    );
    assert_eq!(
        config,
        Config::default(),
        "porecatu.example.toml divergiu de Config::default() -- o errado é o default (ADR-0028)"
    );
}

#[test]
fn example_toml_path_resolves_via_cli_flag() {
    let path = example_toml_path();
    let result = porecatu_config::load(Some(path.as_path()));
    assert!(matches!(result, LoadResult::Loaded { .. }));
    assert_eq!(result.config(), &Config::default());
}
