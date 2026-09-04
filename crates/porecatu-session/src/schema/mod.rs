// SPDX-License-Identifier: GPL-3.0-or-later

//! Despacho por `schema_version` e migração encadeada (ADR-0036 §1).
//! Migrar de v1 para v3 é compor dois passos -- nunca um caminho direto.

pub mod v1;

use serde_json::Value;

pub use v1::SessionFileV1;

/// Versão de schema que este binário grava e sabe ler sem migração.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Um passo de migração de uma versão de schema para a seguinte. Hoje só
/// existe a v1, então a lista de passos em produção está vazia -- o
/// mecanismo nasce testado com uma versão fictícia (ver os testes deste
/// módulo), não só no papel.
pub trait Migration {
    /// Versão de origem que este passo consome.
    fn source_version(&self) -> u32;
    /// Aplica a migração, devolvendo o valor já na forma da próxima versão.
    fn migrate(&self, value: Value) -> Value;
}

/// Resultado de despachar um JSON bruto pela versão.
pub enum Dispatch {
    /// Migrado (ou já na versão atual) e desserializado com sucesso.
    Current(SessionFileV1),
    /// `schema_version` é mais nova que a suportada -- tabela de
    /// recuperação do ADR-0036 §5: não sobrescrever nada.
    Newer(u32),
}

/// Despacha `raw` pela versão, migrando em cadeia até [`CURRENT_SCHEMA_VERSION`].
/// `migrations` é a lista de passos disponíveis; em produção é vazia até
/// existir uma v2 de verdade.
pub fn dispatch(raw: &Value, migrations: &[&dyn Migration]) -> Result<Dispatch, serde_json::Error> {
    let version = raw
        .get("schema_version")
        .and_then(Value::as_u64)
        .map_or(0, |v| v as u32);

    if version > CURRENT_SCHEMA_VERSION {
        return Ok(Dispatch::Newer(version));
    }

    let mut value = raw.clone();
    let mut current_version = version;
    while current_version < CURRENT_SCHEMA_VERSION {
        let Some(step) = migrations
            .iter()
            .find(|m| m.source_version() == current_version)
        else {
            break;
        };
        value = step.migrate(value);
        current_version += 1;
    }

    serde_json::from_value(value).map(Dispatch::Current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Versão fictícia v0 -> v1, só para exercitar a cadeia enquanto não
    /// existe uma v2 de verdade. `v0` aqui não é um schema real do produto
    /// -- é a forma mínima que prova que `dispatch` aplica o passo e
    /// resulta num `SessionFileV1` válido.
    struct FakeV0ToV1;

    impl Migration for FakeV0ToV1 {
        fn source_version(&self) -> u32 {
            0
        }

        fn migrate(&self, mut value: Value) -> Value {
            value["schema_version"] = json!(1);
            value
        }
    }

    #[test]
    fn current_version_deserializes_without_migration() {
        let raw = json!({ "schema_version": 1, "windows": [] });
        match dispatch(&raw, &[]).unwrap() {
            Dispatch::Current(session) => assert_eq!(session.schema_version, 1),
            Dispatch::Newer(_) => panic!("versão atual não deveria ser 'mais nova'"),
        }
    }

    #[test]
    fn newer_version_is_reported_without_deserializing() {
        let raw = json!({ "schema_version": 99, "windows": [] });
        match dispatch(&raw, &[]).unwrap() {
            Dispatch::Newer(found) => assert_eq!(found, 99),
            Dispatch::Current(_) => panic!("schema mais novo não pode virar Current"),
        }
    }

    #[test]
    fn chained_migration_runs_the_fictional_step() {
        let raw = json!({ "schema_version": 0, "windows": [] });
        let migrations: Vec<&dyn Migration> = vec![&FakeV0ToV1];
        match dispatch(&raw, &migrations).unwrap() {
            Dispatch::Current(session) => assert_eq!(session.schema_version, 1),
            Dispatch::Newer(_) => panic!("v0 migrada deveria virar Current na v1"),
        }
    }
}
