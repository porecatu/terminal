// SPDX-License-Identifier: GPL-3.0-or-later

//! O grupo Atalhos (PRD-016 RF-16.28 a RF-16.31, ADR-0058 §5): o que a tela
//! mostra e o que ela grava. Estado puro -- sem `winit`, sem disco --, no
//! molde de `draft.rs`.
//!
//! O que vale é o resultado da resolução em três níveis do ADR-0029 (embutidos
//! → `[keybindings]` comum → `[keybindings.<plataforma>]`), feita pelo
//! **mesmo** [`resolve`] que o terminal usa. A tela só edita a tabela da
//! plataforma em uso, que é a de maior precedência: o que ela escreve ali é
//! exatamente o que passa a valer, e nada muda nas outras plataformas de quem
//! leva o arquivo de uma máquina para outra.
//!
//! A edição trabalha numa **cópia** dessa tabela (`working`); as `Edit`s saem
//! da diferença entre ela e a do arquivo (`base`). Cada operação diz só "esta
//! tecla passa a resolver para esta ação (ou para nenhuma)" -- e a entrada só
//! é escrita se a resolução dos níveis de baixo não já der esse resultado.
//! Daí "restaurar padrão" ser, sem caso especial, a remoção do que a tela
//! tinha escrito, e daí nenhuma tecla ficar com duas ações: o mapa é de tecla
//! para ação.

use std::collections::{BTreeMap, HashMap};

use porecatu_config::{Config, Edit, EditValue, KeyPath, Keybindings};
use porecatu_core::Action;
use porecatu_locale::Catalog;

use super::actions::{Domain, bindable_actions, label};
use crate::keymap::{Chord, Platform, resolve};

/// A tabela de atalhos da plataforma `platform` no `Config`.
fn platform_table(config: &Config, platform: Platform) -> BTreeMap<String, String> {
    match platform {
        Platform::Windows => config.keybindings.windows.clone(),
        Platform::Linux => config.keybindings.linux.clone(),
        Platform::Macos => config.keybindings.macos.clone(),
    }
}

/// O nome da tabela da plataforma no arquivo (`[keybindings.<nome>]`).
fn table_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "windows",
        Platform::Linux => "linux",
        Platform::Macos => "macos",
    }
}

/// A captura em curso numa linha do grupo (RF-16.29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Capturing {
    pub action: Action,
    /// O atalho que a captura substitui; `None` acrescenta um.
    pub replacing: Option<Chord>,
    /// Os atalhos da linha quando a captura começou. A linha em captura
    /// **não muda** com uma recarga de `[keybindings]` (ADR-0059 §3): ela
    /// termina com o que tinha.
    pub frozen: Vec<Chord>,
    /// A combinação capturada já é de outra ação: espera Substituir ou
    /// Cancelar (RF-16.30).
    pub conflict: Option<Conflict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Conflict {
    pub chord: Chord,
    /// A ação que perde a tecla se o usuário escolher Substituir.
    pub other: Action,
}

/// O estado de edição do grupo Atalhos.
#[derive(Debug, Clone)]
pub(crate) struct Shortcuts {
    platform: Platform,
    /// `[keybindings]` comum do arquivo: lido, nunca editado.
    common: BTreeMap<String, String>,
    /// A tabela da plataforma como o arquivo a tem.
    base: BTreeMap<String, String>,
    /// A tabela da plataforma como a tela a deixaria.
    working: BTreeMap<String, String>,
}

impl Shortcuts {
    pub(crate) fn new(config: &Config, platform: Platform) -> Self {
        let table = platform_table(config, platform);
        Self {
            platform,
            common: config.keybindings.common.clone(),
            base: table.clone(),
            working: table,
        }
    }

    #[cfg(test)]
    pub(crate) fn platform(&self) -> Platform {
        self.platform
    }

    // ---- resolução

    /// `Keybindings` com `table` no lugar da tabela da plataforma em uso.
    fn keybindings(
        &self,
        common: &BTreeMap<String, String>,
        table: &BTreeMap<String, String>,
    ) -> Keybindings {
        let mut keybindings = Keybindings {
            common: common.clone(),
            windows: BTreeMap::new(),
            linux: BTreeMap::new(),
            macos: BTreeMap::new(),
        };
        match self.platform {
            Platform::Windows => keybindings.windows = table.clone(),
            Platform::Linux => keybindings.linux = table.clone(),
            Platform::Macos => keybindings.macos = table.clone(),
        }
        keybindings
    }

    fn resolved(
        &self,
        common: &BTreeMap<String, String>,
        table: &BTreeMap<String, String>,
    ) -> HashMap<Chord, Action> {
        resolve(&self.keybindings(common, table), self.platform).bindings
    }

    /// Os atalhos em vigor com a edição aplicada.
    fn effective(&self) -> HashMap<Chord, Action> {
        self.resolved(&self.common, &self.working)
    }

    /// Os atalhos que o arquivo dá, sem a edição.
    fn file_effective(&self) -> HashMap<Chord, Action> {
        self.resolved(&self.common, &self.base)
    }

    /// O que sobra se a tabela da plataforma ficasse vazia: embutidos mais o
    /// comum. É o nível de baixo, contra o qual uma entrada é ou não
    /// necessária.
    fn lower(&self) -> HashMap<Chord, Action> {
        self.resolved(&self.common, &BTreeMap::new())
    }

    /// Só os atalhos embutidos da plataforma: o "padrão" de uma ação.
    fn embedded(&self) -> HashMap<Chord, Action> {
        self.resolved(&BTreeMap::new(), &BTreeMap::new())
    }

    fn chords_in(map: &HashMap<Chord, Action>, action: Action) -> Vec<Chord> {
        let mut chords: Vec<Chord> = map
            .iter()
            .filter(|(_, a)| **a == action)
            .map(|(chord, _)| *chord)
            .collect();
        chords.sort_by_key(Chord::label);
        chords
    }

    // ---- leitura

    /// Os atalhos efetivos de `action` na plataforma em uso, em ordem estável.
    pub(crate) fn chords(&self, action: Action) -> Vec<Chord> {
        Self::chords_in(&self.effective(), action)
    }

    /// Os atalhos embutidos de `action`.
    pub(crate) fn default_chords(&self, action: Action) -> Vec<Chord> {
        Self::chords_in(&self.embedded(), action)
    }

    /// A ação que tem `chord`, se alguma.
    pub(crate) fn holder(&self, chord: Chord) -> Option<Action> {
        self.effective().get(&chord).copied()
    }

    /// Há edição não gravada.
    pub(crate) fn is_dirty(&self) -> bool {
        self.working != self.base
    }

    /// As ações cujos atalhos a edição mudou, na ordem do catálogo.
    pub(crate) fn pending_actions(&self) -> Vec<Action> {
        if !self.is_dirty() {
            return Vec::new();
        }
        let (before, after) = (self.file_effective(), self.effective());
        bindable_actions()
            .into_iter()
            .filter(|action| Self::chords_in(&before, *action) != Self::chords_in(&after, *action))
            .collect()
    }

    /// "Restaurar padrão" está disponível: os atalhos de `action` diferem dos
    /// embutidos.
    pub(crate) fn can_reset(&self, action: Action) -> bool {
        self.chords(action) != self.default_chords(action)
    }

    /// Os atalhos de `action` que o ADR-0008 reserva ao terminal (`Ctrl+<letra>`
    /// sozinho no Windows e no Linux): aceitos, com a advertência (RF-16.29).
    pub(crate) fn reserved_chords(&self, action: Action) -> Vec<Chord> {
        self.chords(action)
            .into_iter()
            .filter(|chord| chord.is_terminal_reserved(self.platform))
            .collect()
    }

    /// As ações do grupo, por domínio, que o filtro deixa passar: pelo nome
    /// legível ou pela tecla (rótulo do chip ou grafia do arquivo), sem
    /// diferenciar maiúsculas. Domínio sem ação some.
    pub(crate) fn rows(&self, catalog: &Catalog, filter: &str) -> Vec<(Domain, Vec<Action>)> {
        let needle = filter.trim().to_lowercase();
        let effective = self.effective();
        Domain::ALL
            .iter()
            .filter_map(|domain| {
                let actions: Vec<Action> = bindable_actions()
                    .into_iter()
                    .filter(|action| Domain::of(*action) == Some(*domain))
                    .filter(|action| {
                        needle.is_empty()
                            || label(catalog, *action)
                                .is_some_and(|name| name.to_lowercase().contains(&needle))
                            || Self::chords_in(&effective, *action).iter().any(|chord| {
                                chord.label().to_lowercase().contains(&needle)
                                    || chord
                                        .to_grammar()
                                        .is_some_and(|text| text.contains(&needle))
                            })
                    })
                    .collect();
                (!actions.is_empty()).then_some((*domain, actions))
            })
            .collect()
    }

    // ---- mudanças

    /// Faz `chord` resolver para `target` (uma ação, ou nenhuma) na tabela
    /// editada: some qualquer grafia dele que já houvesse e escreve a entrada
    /// só se a resolução de baixo não dá esse resultado. Devolve se a tecla
    /// tem grafia na gramática -- uma combinação que não a tem não entra.
    fn set_effective(&mut self, chord: Chord, target: Option<Action>) -> bool {
        let Some(text) = chord.to_grammar() else {
            return false;
        };
        self.working.retain(|key, _| Chord::parse(key) != Ok(chord));
        if target != self.lower().get(&chord).copied() {
            let value = match target {
                Some(action) => action.to_string(),
                None => "none".to_owned(),
            };
            self.working.insert(text, value);
        }
        true
    }

    /// Dá `chord` a `action`, no lugar de `replacing` (ou além dos que ela já
    /// tem, com `None`). Quem tinha `chord` o perde: o mapa é de tecla para
    /// ação, nunca duas. Quem chama já mostrou o conflito (RF-16.30).
    pub(crate) fn bind(&mut self, action: Action, replacing: Option<Chord>, chord: Chord) -> bool {
        if chord.to_grammar().is_none() {
            return false;
        }
        if let Some(old) = replacing
            && old != chord
            && self.holder(old) == Some(action)
        {
            self.set_effective(old, None);
        }
        self.set_effective(chord, Some(action))
    }

    /// Tira `chord` de `action` (`"<tecla>" = "none"`, ou some a entrada que o
    /// dava, o que a resolução pedir).
    pub(crate) fn unbind(&mut self, action: Action, chord: Chord) -> bool {
        if self.holder(chord) != Some(action) {
            return false;
        }
        self.set_effective(chord, None)
    }

    /// Restaurar padrão (RF-16.31): `action` volta a ter exatamente os atalhos
    /// embutidos. O que a tela tinha escrito para ela some da tabela.
    pub(crate) fn reset(&mut self, action: Action) {
        let defaults = self.default_chords(action);
        let embedded = self.embedded();
        let mut released = Vec::new();
        for chord in self.chords(action) {
            if !defaults.contains(&chord) {
                self.set_effective(chord, None);
                released.push(chord);
            }
        }
        for chord in defaults {
            self.set_effective(chord, Some(action));
        }
        // Uma tecla que `action` tinha tomado de outra (Substituir) volta à
        // dona de fábrica: restaurar uma ação não deixa a outra sem o atalho
        // que perdeu por causa dela. Só se a tecla ficou livre.
        for chord in released {
            if let Some(owner) = embedded.get(&chord).copied()
                && owner != action
                && self.holder(chord).is_none()
            {
                self.set_effective(chord, Some(owner));
            }
        }
    }

    // ---- gravação

    /// As edições a gravar: **só** em `[keybindings.<plataforma em uso>]`.
    /// Primeiro as remoções, depois as entradas novas ou mudadas.
    pub(crate) fn edits(&self) -> Vec<Edit> {
        let path = |key: &str| {
            KeyPath::new(["keybindings", table_name(self.platform), key]).expect("três segmentos")
        };
        let mut edits = Vec::new();
        for key in self.base.keys() {
            if !self.working.contains_key(key) {
                edits.push(Edit::Remove(path(key)));
            }
        }
        for (key, value) in &self.working {
            if self.base.get(key) != Some(value) {
                edits.push(Edit::Set(path(key), EditValue::String(value.clone())));
            }
        }
        edits
    }

    /// Descartar: de volta ao que o arquivo diz.
    pub(crate) fn discard(&mut self) {
        self.working = self.base.clone();
    }

    /// Salvar deu certo: o arquivo agora diz `saved`.
    pub(crate) fn commit(&mut self, saved: &Config) {
        self.common = saved.keybindings.common.clone();
        self.base = platform_table(saved, self.platform);
        self.working = self.base.clone();
    }

    /// O arquivo mudou por outro caminho. Sem edição a tela acompanha; com
    /// edição ela fica como está, só com a base nova. Quem decide **se** isto
    /// roda é `FileState` (RF-16.23): a faixa de conflito pergunta antes.
    pub(crate) fn rebase(&mut self, config: &Config) {
        let clean = !self.is_dirty();
        self.common = config.keybindings.common.clone();
        self.base = platform_table(config, self.platform);
        if clean {
            self.working = self.base.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use porecatu_config::ConfigDocument;
    use winit::keyboard::{Key, NamedKey};

    use super::*;
    use crate::messages::test_support;
    use porecatu_term::Modifiers;

    const EXAMPLE: &str = include_str!("../../../../docs/config/porecatu.example.toml");

    fn shortcuts_for(text: &str, platform: Platform) -> Shortcuts {
        Shortcuts::new(&porecatu_config::parse(text).unwrap().0, platform)
    }

    fn example(platform: Platform) -> Shortcuts {
        shortcuts_for(EXAMPLE, platform)
    }

    fn chord(text: &str) -> Chord {
        Chord::parse(text).unwrap()
    }

    /// Aplica as edições ao texto, relê pelo `parse` da carga e resolve.
    fn applied(shortcuts: &Shortcuts, text: &str) -> (String, Shortcuts) {
        let document = ConfigDocument::parse(text).unwrap();
        let (after, _) = document.apply_checked(&shortcuts.edits()).unwrap();
        let reread = shortcuts_for(&after, shortcuts.platform());
        (after, reread)
    }

    fn event_chord(key: Key, ctrl: bool, shift: bool) -> Chord {
        Chord::from_key(
            &key,
            Modifiers {
                ctrl,
                alt: false,
                shift,
                super_: false,
            },
        )
        .unwrap()
    }

    // ---- leitura

    #[test]
    fn the_effective_shortcuts_are_the_three_level_resolution_not_just_the_file() {
        let s = example(Platform::Windows);
        assert_eq!(s.chords(Action::TabNew), [chord("ctrl+shift+t")]);
        assert_eq!(s.chords(Action::SearchOpen), [chord("ctrl+shift+f")]);
        // `group.new_tab` não tem padrão em nenhuma plataforma.
        assert!(s.chords(Action::GroupNewTab).is_empty());
        // A tabela da plataforma é a de maior precedência.
        let s = shortcuts_for(
            "[keybindings.windows]\n\"ctrl+shift+t\" = \"none\"\n\"ctrl+shift+j\" = \"tab.new\"\n",
            Platform::Windows,
        );
        assert_eq!(s.chords(Action::TabNew), [chord("ctrl+shift+j")]);
        // A de outra plataforma não vale aqui.
        let s = shortcuts_for(
            "[keybindings.linux]\n\"ctrl+shift+t\" = \"none\"\n",
            Platform::Windows,
        );
        assert_eq!(s.chords(Action::TabNew), [chord("ctrl+shift+t")]);
    }

    #[test]
    fn macos_reads_its_own_embedded_defaults() {
        let s = example(Platform::Macos);
        assert!(s.chords(Action::SettingsOpen).contains(&chord("cmd+comma")));
        let s = example(Platform::Windows);
        assert_eq!(s.chords(Action::SettingsOpen), [chord("ctrl+shift+o")]);
    }

    #[test]
    fn rows_group_by_domain_in_catalog_order_and_skip_empty_domains() {
        let s = example(Platform::Windows);
        let catalog = test_support::pt_br();
        let rows = s.rows(&catalog, "");
        let domains: Vec<Domain> = rows.iter().map(|(d, _)| *d).collect();
        assert_eq!(domains, Domain::ALL);
        let total: usize = rows.iter().map(|(_, a)| a.len()).sum();
        assert_eq!(total, bindable_actions().len());
        assert_eq!(rows[0].1[0], Action::TabNew);
    }

    #[test]
    fn the_filter_matches_the_name_or_the_key_ignoring_case() {
        let s = example(Platform::Windows);
        let catalog = test_support::pt_br();
        let names = |filter: &str| -> Vec<Action> {
            s.rows(&catalog, filter)
                .into_iter()
                .flat_map(|(_, actions)| actions)
                .collect()
        };
        assert_eq!(names("ir para a aba 3"), [Action::TabGoto(3)]);
        assert_eq!(names("IR PARA A ABA 3"), [Action::TabGoto(3)]);
        assert!(names("nova aba").contains(&Action::TabNew));
        // Pela tecla: o rótulo do chip e a grafia do arquivo.
        assert!(names("shift+f3").contains(&Action::SearchPrev));
        assert!(names("ctrl+shift+f").contains(&Action::SearchOpen));
        assert!(names("pagedown").contains(&Action::GroupNext));
        assert!(names("zzzz").is_empty());
        // Um domínio sem nenhuma linha que case não aparece.
        let rows = s.rows(&catalog, "ir para a aba 3");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, Domain::Tab);
    }

    // ---- ida e volta: capturar -> edições -> arquivo -> resolução -> tecla

    #[test]
    fn a_captured_shortcut_survives_the_file_and_matches_the_same_key_event() {
        let mut s = example(Platform::Windows);
        // `search.open` -> `Ctrl+Shift+J`, uma tecla livre.
        let captured = event_chord(Key::Character("J".into()), true, true);
        assert_eq!(s.holder(captured), None);
        assert!(s.bind(Action::SearchOpen, Some(chord("ctrl+shift+f")), captured));
        let (text, reread) = applied(&s, EXAMPLE);
        assert!(text.contains("[keybindings.windows]"));
        assert!(text.contains("\"ctrl+shift+j\" = \"search.open\""));
        assert_eq!(reread.chords(Action::SearchOpen), [captured]);
        // A tecla antiga não é mais da busca.
        assert_eq!(reread.holder(chord("ctrl+shift+f")), None);
        // E o evento de teclado de verdade casa o atalho gravado.
        let again = event_chord(Key::Character("j".into()), true, true);
        assert_eq!(reread.holder(again), Some(Action::SearchOpen));
    }

    #[test]
    fn every_bindable_action_round_trips_a_new_key_through_the_file() {
        for action in bindable_actions() {
            let mut s = example(Platform::Windows);
            let free = chord("ctrl+alt+shift+f12");
            assert!(s.bind(action, None, free), "{action}");
            let (_, reread) = applied(&s, EXAMPLE);
            assert_eq!(reread.holder(free), Some(action), "{action}");
        }
    }

    #[test]
    fn a_shifted_symbol_goes_through_the_grammar_word() {
        let mut s = example(Platform::Windows);
        let key = event_chord(Key::Character(",".into()), true, true);
        assert!(s.bind(Action::TabNew, None, key));
        let (text, reread) = applied(&s, EXAMPLE);
        assert!(text.contains("ctrl+shift+comma"));
        // `config.reload` tinha essa tecla: perdeu.
        assert_eq!(reread.holder(key), Some(Action::TabNew));
        assert!(reread.chords(Action::ConfigReload).is_empty());
    }

    // ---- conflito

    #[test]
    fn replacing_a_conflicting_key_leaves_it_with_one_action_only() {
        let mut s = example(Platform::Windows);
        let key = chord("ctrl+shift+r");
        // `tab.rename` tem `Ctrl+Shift+R` de fábrica.
        assert_eq!(s.holder(key), Some(Action::TabRename));
        assert!(s.bind(Action::SearchOpen, Some(chord("ctrl+shift+f")), key));
        assert_eq!(s.holder(key), Some(Action::SearchOpen));
        assert!(s.chords(Action::TabRename).is_empty());
        // Nenhuma tecla do mapa tem duas ações, por construção -- e a do
        // arquivo relido concorda.
        let (_, reread) = applied(&s, EXAMPLE);
        assert_eq!(reread.holder(key), Some(Action::SearchOpen));
        assert!(reread.chords(Action::TabRename).is_empty());
        let mut seen = std::collections::HashSet::new();
        for action in bindable_actions() {
            for c in reread.chords(action) {
                assert!(seen.insert(c), "{} com duas ações", c.label());
            }
        }
    }

    #[test]
    fn a_conflict_is_a_key_held_by_another_action_never_by_the_same_one() {
        let s = example(Platform::Windows);
        assert_eq!(s.holder(chord("ctrl+shift+r")), Some(Action::TabRename));
        assert_eq!(s.holder(chord("ctrl+shift+j")), None);
    }

    // ---- remover

    #[test]
    fn removing_a_default_writes_none_and_removing_a_user_key_drops_the_entry() {
        let mut s = example(Platform::Windows);
        assert!(s.unbind(Action::TabRename, chord("ctrl+shift+r")));
        assert!(s.chords(Action::TabRename).is_empty());
        let edits = s.edits();
        assert_eq!(edits.len(), 1);
        let Edit::Set(path, EditValue::String(value)) = &edits[0] else {
            panic!("{edits:?}")
        };
        assert_eq!(path.to_string(), "keybindings.windows.\"ctrl+shift+r\"");
        assert_eq!(value, "none");
        let (_, reread) = applied(&s, EXAMPLE);
        assert!(reread.chords(Action::TabRename).is_empty());

        // Uma tecla que a própria tela escreveu: tirá-la remove a entrada, não
        // escreve `none` por cima.
        let mut s = example(Platform::Windows);
        s.bind(Action::GroupNewTab, None, chord("ctrl+shift+j"));
        assert!(s.is_dirty());
        s.unbind(Action::GroupNewTab, chord("ctrl+shift+j"));
        assert!(!s.is_dirty());
        assert!(s.edits().is_empty());
    }

    #[test]
    fn an_action_with_two_default_keys_loses_each_with_its_own_none() {
        // O padrão de `tab.goto_1` é um só; dá-se um segundo e tiram-se os dois.
        let mut s = example(Platform::Windows);
        s.bind(Action::TabGoto(1), None, chord("ctrl+shift+j"));
        assert_eq!(s.chords(Action::TabGoto(1)).len(), 2);
        for c in s.chords(Action::TabGoto(1)) {
            s.unbind(Action::TabGoto(1), c);
        }
        assert!(s.chords(Action::TabGoto(1)).is_empty());
        let (_, reread) = applied(&s, EXAMPLE);
        assert!(reread.chords(Action::TabGoto(1)).is_empty());
    }

    // ---- restaurar padrão

    #[test]
    fn restore_default_goes_back_to_the_embedded_shortcut_and_clears_what_the_screen_wrote() {
        let mut s = example(Platform::Windows);
        let defaults = s.default_chords(Action::TabNew);
        assert_eq!(defaults, [chord("ctrl+shift+t")]);
        assert!(!s.can_reset(Action::TabNew));
        s.bind(
            Action::TabNew,
            Some(chord("ctrl+shift+t")),
            chord("ctrl+shift+j"),
        );
        assert!(s.can_reset(Action::TabNew));
        s.reset(Action::TabNew);
        assert_eq!(s.chords(Action::TabNew), defaults);
        assert!(!s.can_reset(Action::TabNew));
        // Nada sobrou na tabela: a edição desfeita não é pendência.
        assert!(!s.is_dirty());
        assert!(s.edits().is_empty());
    }

    #[test]
    fn restore_default_also_gives_back_a_key_removed_with_none() {
        let mut s = example(Platform::Windows);
        s.unbind(Action::TabRename, chord("ctrl+shift+r"));
        assert!(s.can_reset(Action::TabRename));
        s.reset(Action::TabRename);
        assert_eq!(s.chords(Action::TabRename), [chord("ctrl+shift+r")]);
        assert!(s.edits().is_empty());
    }

    #[test]
    fn restoring_an_action_that_took_a_key_gives_it_back_to_its_factory_owner() {
        let mut s = example(Platform::Windows);
        s.bind(
            Action::SearchOpen,
            Some(chord("ctrl+shift+f")),
            chord("ctrl+shift+r"),
        );
        assert!(s.chords(Action::TabRename).is_empty());
        // Restaurar a busca devolve `Ctrl+Shift+F` a ela e `Ctrl+Shift+R` a
        // `tab.rename`: nenhuma das duas fica sem o atalho de fábrica.
        s.reset(Action::SearchOpen);
        assert_eq!(s.chords(Action::SearchOpen), [chord("ctrl+shift+f")]);
        assert_eq!(s.chords(Action::TabRename), [chord("ctrl+shift+r")]);
        assert!(!s.is_dirty());
        assert!(s.edits().is_empty());
    }

    #[test]
    fn a_released_key_stays_free_if_someone_else_took_it_meanwhile() {
        let mut s = example(Platform::Windows);
        s.bind(
            Action::SearchOpen,
            Some(chord("ctrl+shift+f")),
            chord("ctrl+shift+r"),
        );
        // `group.new_tab` fica com a tecla que `search.open` solta no reset?
        // Não: o reset só devolve à dona de fábrica se a tecla ficou livre.
        s.reset(Action::SearchOpen);
        s.bind(Action::GroupNewTab, None, chord("ctrl+shift+r"));
        assert_eq!(s.holder(chord("ctrl+shift+r")), Some(Action::GroupNewTab));
        s.reset(Action::SearchOpen);
        assert_eq!(s.holder(chord("ctrl+shift+r")), Some(Action::GroupNewTab));
    }

    #[test]
    fn restore_default_wins_over_a_key_the_common_table_took_away() {
        let mut s = shortcuts_for(
            "[keybindings]\n\"ctrl+shift+t\" = \"none\"\n",
            Platform::Windows,
        );
        assert!(s.chords(Action::TabNew).is_empty());
        assert!(s.can_reset(Action::TabNew));
        s.reset(Action::TabNew);
        assert_eq!(s.chords(Action::TabNew), [chord("ctrl+shift+t")]);
        // Como o comum não é editado, é a tabela da plataforma que o cobre.
        let edits = s.edits();
        assert_eq!(edits.len(), 1);
        assert!(matches!(&edits[0], Edit::Set(path, _)
            if path.to_string() == "keybindings.windows.\"ctrl+shift+t\""));
    }

    // ---- só a tabela da plataforma em uso

    #[test]
    fn edits_only_ever_touch_the_table_of_the_platform_in_use() {
        for (platform, name) in [
            (Platform::Windows, "windows"),
            (Platform::Linux, "linux"),
            (Platform::Macos, "macos"),
        ] {
            let mut s = example(platform);
            s.bind(Action::TabNew, None, chord("ctrl+shift+j"));
            s.unbind(Action::TabRename, chord("ctrl+shift+r"));
            s.reset(Action::TabNew);
            s.bind(Action::SearchOpen, None, chord("ctrl+shift+k"));
            for edit in s.edits() {
                let path = match &edit {
                    Edit::Set(path, _) | Edit::Remove(path) => path,
                };
                assert_eq!(path.segments().len(), 3);
                assert_eq!(path.segments()[0], "keybindings");
                assert_eq!(path.segments()[1], name, "{edit:?}");
            }
        }
    }

    #[test]
    fn the_common_table_and_the_other_platforms_stay_byte_for_byte() {
        let text = "# topo\n[keybindings]\n\"ctrl+shift+t\" = \"tab.new\" # comum\n\n[keybindings.linux]\n\"ctrl+shift+x\" = \"tab.close\"\n";
        let mut s = shortcuts_for(text, Platform::Windows);
        s.bind(
            Action::SearchOpen,
            Some(chord("ctrl+shift+f")),
            chord("ctrl+shift+j"),
        );
        let (after, _) = applied(&s, text);
        assert!(after.starts_with(
            "# topo\n[keybindings]\n\"ctrl+shift+t\" = \"tab.new\" # comum\n\n[keybindings.linux]\n\"ctrl+shift+x\" = \"tab.close\"\n"
        ), "{after}");
        assert!(after.contains("[keybindings.windows]"));
    }

    #[test]
    fn a_different_spelling_of_the_same_key_is_replaced_not_duplicated() {
        let text = "[keybindings.windows]\n\"Ctrl+Shift+J\" = \"tab.new\"\n";
        let mut s = shortcuts_for(text, Platform::Windows);
        s.bind(Action::SearchOpen, None, chord("ctrl+shift+j"));
        let (after, reread) = applied(&s, text);
        // Uma entrada só para a tecla -- a duplicata seria erro de carga.
        assert!(porecatu_config::parse(&after).is_ok());
        assert_eq!(
            reread.holder(chord("ctrl+shift+j")),
            Some(Action::SearchOpen)
        );
        assert_eq!(
            after.matches("shift+j").count() + after.matches("Shift+J").count(),
            1
        );
    }

    // ---- gravação, descarte, recarga

    #[test]
    fn nothing_pending_means_no_edits_and_discard_goes_back() {
        let mut s = example(Platform::Windows);
        assert!(!s.is_dirty());
        assert!(s.edits().is_empty());
        s.bind(Action::TabNew, None, chord("ctrl+shift+j"));
        assert!(s.is_dirty());
        s.discard();
        assert!(!s.is_dirty());
        assert!(s.edits().is_empty());
        assert_eq!(s.chords(Action::TabNew), [chord("ctrl+shift+t")]);
    }

    #[test]
    fn pending_actions_name_every_action_the_edit_touched() {
        let mut s = example(Platform::Windows);
        assert!(s.pending_actions().is_empty());
        s.bind(
            Action::SearchOpen,
            Some(chord("ctrl+shift+f")),
            chord("ctrl+shift+r"),
        );
        assert_eq!(
            s.pending_actions(),
            [Action::TabRename, Action::SearchOpen],
            "quem ganhou e quem perdeu, na ordem do catálogo"
        );
    }

    #[test]
    fn commit_makes_the_saved_file_the_new_base() {
        let mut s = example(Platform::Windows);
        s.bind(
            Action::SearchOpen,
            Some(chord("ctrl+shift+f")),
            chord("ctrl+shift+j"),
        );
        let document = ConfigDocument::parse(EXAMPLE).unwrap();
        let (saved_text, _) = document.apply_checked(&s.edits()).unwrap();
        let saved = porecatu_config::parse(&saved_text).unwrap().0;
        s.commit(&saved);
        assert!(!s.is_dirty());
        assert_eq!(s.chords(Action::SearchOpen), [chord("ctrl+shift+j")]);
    }

    #[test]
    fn a_reload_without_edits_follows_the_file_and_with_edits_keeps_them() {
        let mut s = example(Platform::Windows);
        let changed = porecatu_config::parse(
            "[keybindings.windows]\n\"ctrl+shift+j\" = \"tab.new\"\n\"ctrl+shift+t\" = \"none\"\n",
        )
        .unwrap()
        .0;
        s.rebase(&changed);
        assert_eq!(s.chords(Action::TabNew), [chord("ctrl+shift+j")]);
        assert!(!s.is_dirty());

        let mut s = example(Platform::Windows);
        s.bind(
            Action::SearchOpen,
            Some(chord("ctrl+shift+f")),
            chord("ctrl+shift+k"),
        );
        s.rebase(&changed);
        assert_eq!(s.chords(Action::SearchOpen), [chord("ctrl+shift+k")]);
        assert!(s.is_dirty());
    }

    // ---- teclas que a gramática não escreve

    #[test]
    fn a_key_the_grammar_cannot_write_is_refused() {
        let mut s = example(Platform::Windows);
        let plus = event_chord(Key::Character("+".into()), true, false);
        assert!(!s.bind(Action::TabNew, None, plus));
        assert!(!s.is_dirty());
    }

    #[test]
    fn a_reserved_key_is_accepted_and_reported() {
        let mut s = example(Platform::Windows);
        let reserved = chord("ctrl+r");
        assert!(s.bind(Action::SearchOpen, Some(chord("ctrl+shift+f")), reserved));
        assert_eq!(s.reserved_chords(Action::SearchOpen), [reserved]);
        assert!(s.reserved_chords(Action::TabNew).is_empty());
        let (_, reread) = applied(&s, EXAMPLE);
        assert_eq!(reread.holder(reserved), Some(Action::SearchOpen));
        // No macOS `Ctrl+letra` é livre do terminal para o app, sem advertência.
        let mut mac = example(Platform::Macos);
        mac.bind(Action::SearchOpen, None, reserved);
        assert!(mac.reserved_chords(Action::SearchOpen).is_empty());
    }

    #[test]
    fn named_keys_round_trip_too() {
        let mut s = example(Platform::Windows);
        let key = event_chord(Key::Named(NamedKey::F9), true, false);
        assert!(s.bind(Action::TabNew, None, key));
        let (text, reread) = applied(&s, EXAMPLE);
        assert!(text.contains("ctrl+f9"));
        assert_eq!(reread.holder(key), Some(Action::TabNew));
    }
}
