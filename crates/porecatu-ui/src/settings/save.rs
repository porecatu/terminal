// SPDX-License-Identifier: GPL-3.0-or-later

//! Salvar (RF-16.14, RF-16.19, ADR-0058): aplica as edições do rascunho sobre
//! o texto que está no disco e grava, atomicamente, no alvo real do arquivo.
//! Não toca em `Config` nem em janela: devolve o `Config` que o texto gravado
//! diz, e quem chama decide o que fazer com ele. A recarga a quente é quem
//! aplica a mudança ao app (ADR-0058 §6), nunca esta função.
//!
//! O arquivo é lido **na hora de salvar**: o texto lido é a base da edição e
//! a conferência de "mudou no disco" do `ConfigDocument::save` só pega uma
//! corrida entre a leitura e a gravação.
// TODO(tarefa 11): estados do arquivo. Hoje se assume o arquivo existente e
// válido; falta o arquivo inexistente (RF-16.21, base = o exemplo embutido), o
// inválido (RF-16.22, tela somente leitura) e o alterado fora com a tela aberta
// (RF-16.23, a base lida na abertura e a faixa Recarregar / Manter).

use std::fs;
use std::path::{Path, PathBuf};

use porecatu_config::{Config, ConfigDocument, Edit, EditError};

/// Por que o Salvar não gravou. Nada foi escrito em nenhum dos casos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveError {
    /// O arquivo não pôde ser lido. `cause` é o texto do sistema.
    Read { path: PathBuf, cause: String },
    /// A edição não se aplica, o texto resultante seria recusado pelo
    /// carregador, ou o disco falhou.
    Edit(EditError),
}

/// Grava `edits` em `path` e devolve o `Config` do texto gravado.
pub(crate) fn save(path: &Path, edits: &[Edit]) -> Result<Config, SaveError> {
    let text = fs::read_to_string(path).map_err(|err| SaveError::Read {
        path: path.to_path_buf(),
        cause: err.to_string(),
    })?;
    let mut document = ConfigDocument::parse(&text).map_err(SaveError::Edit)?;
    document.save(path, edits).map_err(SaveError::Edit)?;
    // O texto gravado já passou pelo `parse` da carga dentro de `save`.
    porecatu_config::parse(document.base())
        .map(|(config, _)| config)
        .map_err(|err| SaveError::Edit(EditError::Invalid(err)))
}

#[cfg(test)]
mod tests {
    use porecatu_config::{EditValue, KeyPath};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "porecatu-settings-save-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn set(path: &str, value: EditValue) -> Edit {
        Edit::Set(KeyPath::parse(path).unwrap(), value)
    }

    #[test]
    fn writes_only_the_edited_keys_and_returns_the_saved_config() {
        let path = scratch("only-edited.toml");
        let original = "# meu arquivo\n[terminal.font]\nsize = 14.0   # RF-5.3\n\n[terminal.selection]\ncopy_on_select = false\n";
        fs::write(&path, original).unwrap();

        let config = save(
            &path,
            &[
                set("terminal.font.size", EditValue::Float(16.0)),
                set("terminal.selection.copy_on_select", EditValue::Bool(true)),
            ],
        )
        .unwrap();

        assert_eq!(config.terminal.font.size, 16.0);
        assert!(config.terminal.selection.copy_on_select);
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(
            written,
            "# meu arquivo\n[terminal.font]\nsize = 16.0   # RF-5.3\n\n[terminal.selection]\ncopy_on_select = true\n"
        );
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_remove_drops_the_key_and_keeps_the_comment_above_it() {
        let path = scratch("remove.toml");
        fs::write(
            &path,
            "[terminal.font]\n# tamanho em pixels\nsize = 16.0\nfamily = \"X\"\n",
        )
        .unwrap();

        let config = save(
            &path,
            &[Edit::Remove(KeyPath::parse("terminal.font.size").unwrap())],
        )
        .unwrap();

        assert_eq!(
            config.terminal.font.size,
            Config::default().terminal.font.size
        );
        let written = fs::read_to_string(&path).unwrap();
        assert!(!written.contains("size = 16.0"));
        assert!(written.contains("# tamanho em pixels"));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_text_the_loader_would_refuse_is_never_written() {
        let path = scratch("refused.toml");
        fs::write(&path, "[terminal]\n").unwrap();
        let error = save(
            &path,
            &[set("terminal.font.size", EditValue::String("x".to_owned()))],
        )
        .unwrap_err();
        assert!(matches!(error, SaveError::Edit(EditError::Invalid(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[terminal]\n");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_file_is_a_read_error_and_nothing_is_created() {
        let path = scratch("missing.toml");
        let error = save(
            &path,
            &[set(
                "terminal.selection.copy_on_select",
                EditValue::Bool(true),
            )],
        )
        .unwrap_err();
        assert!(matches!(error, SaveError::Read { .. }));
        assert!(!path.exists());
    }
}
