// SPDX-License-Identifier: GPL-3.0-or-later

//! Salvar (RF-16.14, RF-16.19, RF-16.21, ADR-0058): aplica as edições do
//! rascunho sobre o texto **que a tela viu** e grava, atomicamente, no alvo
//! real do arquivo. Não toca em `Config` nem em janela: devolve o texto
//! gravado e o `Config` dele, e quem chama decide o que fazer. A recarga a
//! quente é quem aplica a mudança ao app (ADR-0058 §6), nunca esta função.
//!
//! A base é a da tela (`FileState::base`), não uma leitura nova: o
//! `ConfigDocument::save` confere, na hora de gravar, que o arquivo ainda é a
//! base, e devolve `EditError::Changed` se alguém o mexeu por fora (RF-16.23).
//! Arquivo inexistente tem base `None`, e o Salvar parte do exemplo embutido,
//! para quem um dia abrir o arquivo encontrar a documentação de cada chave
//! (RF-16.21).

use std::path::Path;

use porecatu_config::{Config, ConfigDocument, Edit, EditError};

/// Por que o Salvar não gravou. Nada foi escrito em nenhum dos casos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveError {
    /// A edição não se aplica, o texto resultante seria recusado pelo
    /// carregador, o arquivo mudou por fora desde a base (`Changed`), ou o
    /// disco falhou.
    Edit(EditError),
}

impl SaveError {
    /// O arquivo mudou no disco desde o que a tela viu: quem chama relê o
    /// disco e mostra a faixa de conflito (RF-16.23).
    pub(crate) fn is_conflict(&self) -> bool {
        matches!(self, SaveError::Edit(EditError::Changed { .. }))
    }
}

/// O que um Salvar bem-sucedido deixou no disco.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Saved {
    /// O texto gravado: a base nova da tela.
    pub text: String,
    /// O `Config` desse texto.
    pub config: Config,
}

/// Grava `edits` em `path` sobre `base` -- o texto que a tela viu, ou `None`
/// para um arquivo que não existia, e aí sobre `template`, o exemplo embutido.
pub(crate) fn save(
    path: &Path,
    base: Option<&str>,
    template: &str,
    edits: &[Edit],
) -> Result<Saved, SaveError> {
    let mut document = match base {
        Some(text) => ConfigDocument::parse(text),
        None => ConfigDocument::from_template(template),
    }
    .map_err(SaveError::Edit)?;
    document.save(path, edits).map_err(SaveError::Edit)?;
    // O texto gravado já passou pelo `parse` da carga dentro de `save`.
    let text = document.base().to_owned();
    porecatu_config::parse(&text)
        .map(|(config, _)| Saved { text, config })
        .map_err(|err| SaveError::Edit(EditError::Invalid(err)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use porecatu_config::{EditValue, KeyPath};

    use super::*;

    const EXAMPLE: &str = include_str!("../../../../docs/config/porecatu.example.toml");

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

    /// Escreve `text` e devolve o caminho: o arquivo como a tela o viu.
    fn file_with(name: &str, text: &str) -> PathBuf {
        let path = scratch(name);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn writes_only_the_edited_keys_and_returns_the_saved_config() {
        let original = "# meu arquivo\n[terminal.font]\nsize = 14.0   # RF-5.3\n\n[terminal.selection]\ncopy_on_select = false\n";
        let path = file_with("only-edited.toml", original);

        let saved = save(
            &path,
            Some(original),
            EXAMPLE,
            &[
                set("terminal.font.size", EditValue::Float(16.0)),
                set("terminal.selection.copy_on_select", EditValue::Bool(true)),
            ],
        )
        .unwrap();

        assert_eq!(saved.config.terminal.font.size, 16.0);
        assert!(saved.config.terminal.selection.copy_on_select);
        let expected = "# meu arquivo\n[terminal.font]\nsize = 16.0   # RF-5.3\n\n[terminal.selection]\ncopy_on_select = true\n";
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(saved.text, expected, "o texto gravado é a base nova");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_remove_drops_the_key_and_keeps_the_comment_above_it() {
        let original = "[terminal.font]\n# tamanho em pixels\nsize = 16.0\nfamily = \"X\"\n";
        let path = file_with("remove.toml", original);
        let saved = save(
            &path,
            Some(original),
            EXAMPLE,
            &[Edit::Remove(KeyPath::parse("terminal.font.size").unwrap())],
        )
        .unwrap();

        assert_eq!(
            saved.config.terminal.font.size,
            Config::default().terminal.font.size
        );
        let written = fs::read_to_string(&path).unwrap();
        assert!(!written.contains("size = 16.0"));
        assert!(written.contains("# tamanho em pixels"));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_text_the_loader_would_refuse_is_never_written() {
        let path = file_with("refused.toml", "[terminal]\n");
        let error = save(
            &path,
            Some("[terminal]\n"),
            EXAMPLE,
            &[set("terminal.font.size", EditValue::String("x".to_owned()))],
        )
        .unwrap_err();
        assert!(matches!(error, SaveError::Edit(EditError::Invalid(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[terminal]\n");
        fs::remove_file(&path).unwrap();
    }

    // ---- arquivo inexistente (RF-16.21)

    #[test]
    fn a_missing_file_is_created_from_the_template_plus_the_edits() {
        let path = scratch("created.toml");
        assert!(!path.exists());
        let saved = save(
            &path,
            None,
            EXAMPLE,
            &[set("terminal.font.size", EditValue::Float(16.0))],
        )
        .unwrap();
        assert_eq!(saved.config.terminal.font.size, 16.0);
        let written = fs::read_to_string(&path).unwrap();
        // O arquivo é o exemplo, com a documentação de cada chave, mais a
        // mudança -- e só ela.
        let expected = ConfigDocument::from_template(EXAMPLE)
            .unwrap()
            .apply(&[set("terminal.font.size", EditValue::Float(16.0))])
            .unwrap();
        assert_eq!(written, expected);
        assert!(written.contains("RF-5.3"), "a documentação do exemplo veio");
        assert!(written.contains("size = 16.0"));
        assert_eq!(saved.text, written);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_file_save_creates_the_missing_directory_too() {
        let dir = scratch("dir");
        let path = dir.join("nested").join("porecatu.toml");
        save(
            &path,
            None,
            EXAMPLE,
            &[set("terminal.font.size", EditValue::Float(15.0))],
        )
        .unwrap();
        assert!(path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_that_appeared_while_the_screen_thought_it_missing_is_a_conflict() {
        let path = file_with("appeared.toml", "[terminal.font]\nsize = 20.0\n");
        let error = save(
            &path,
            None,
            EXAMPLE,
            &[set("terminal.font.size", EditValue::Float(16.0))],
        )
        .unwrap_err();
        assert!(error.is_conflict());
        // Nada foi gravado por cima do que apareceu.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[terminal.font]\nsize = 20.0\n"
        );
        fs::remove_file(&path).unwrap();
    }

    // ---- alterado por fora (RF-16.23)

    #[test]
    fn a_file_changed_outside_since_the_base_is_a_conflict_and_nothing_is_written() {
        let base = "[terminal.font]\nsize = 14.0\n";
        let outside = "[terminal.font]\nsize = 14.0\n[terminal]\ntheme = \"nord\"\n";
        let path = file_with("outside.toml", outside);
        let error = save(
            &path,
            Some(base),
            EXAMPLE,
            &[set("terminal.font.size", EditValue::Float(18.0))],
        )
        .unwrap_err();
        assert!(error.is_conflict());
        assert_eq!(fs::read_to_string(&path).unwrap(), outside);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn keeping_my_changes_saves_the_pending_edit_on_top_of_the_outside_change() {
        // A tela viu `base`; por fora o tema mudou; a pendência é o tamanho da
        // fonte. Manter troca a base pelo texto novo e grava: as duas mudanças.
        let base = "[terminal.font]\nsize = 14.0\n";
        let outside = "[terminal.font]\nsize = 14.0\n\n[terminal]\ntheme = \"nord\"\n";
        let path = file_with("keep.toml", outside);
        let edits = [set("terminal.font.size", EditValue::Float(18.0))];

        assert!(
            save(&path, Some(base), EXAMPLE, &edits)
                .unwrap_err()
                .is_conflict()
        );
        // "Manter": a base é o que está no disco agora.
        let saved = save(&path, Some(outside), EXAMPLE, &edits).unwrap();

        assert_eq!(saved.config.terminal.font.size, 18.0);
        assert_eq!(saved.config.terminal.theme, "nord");
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(
            written,
            "[terminal.font]\nsize = 18.0\n\n[terminal]\ntheme = \"nord\"\n"
        );
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn saving_twice_in_a_row_never_conflicts_with_itself() {
        let original = "[terminal.font]\nsize = 14.0\n";
        let path = file_with("twice.toml", original);
        let first = save(
            &path,
            Some(original),
            EXAMPLE,
            &[set("terminal.font.size", EditValue::Float(15.0))],
        )
        .unwrap();
        // A tela guardou o texto gravado como base.
        let second = save(
            &path,
            Some(&first.text),
            EXAMPLE,
            &[set("terminal.font.size", EditValue::Float(16.0))],
        )
        .unwrap();
        assert_eq!(second.config.terminal.font.size, 16.0);
        fs::remove_file(&path).unwrap();
    }
}
