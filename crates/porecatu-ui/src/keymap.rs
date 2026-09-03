// SPDX-License-Identifier: GPL-3.0-or-later

//! Gramática de tecla e resolução de `[keybindings]` (ADR-0029). `Chord`
//! e a resolução vivem em `porecatu-ui`, não em `porecatu-core`: casar
//! contra `winit::keyboard::Key` é vocabulário de GUI, e `porecatu-core`
//! não conhece `winit` (só `Action`, vocabulário de domínio, mora lá).
//!
//! `Chord` é um struct de flags booleanas (`ctrl`/`alt`/`shift`/`cmd`) mais
//! a tecla -- a canonicalização que o ADR pede ("ordene os modificadores")
//! sai de graça dessa representação: `"shift+ctrl+t"` e `"ctrl+shift+t"`
//! produzem o mesmo `Chord` porque a ordem de leitura nunca importa para
//! um conjunto de flags, só o valor final de cada uma.

use std::collections::{BTreeMap, HashMap};

use porecatu_core::Action;
use porecatu_term::Modifiers;
use winit::keyboard::{Key, NamedKey};

/// Uma tecla nomeada (`equals`, `pageup`, ...) ou o próprio caractere
/// (`t`, `1`). Símbolos ambíguos em prosa (`=`, `,`, `` ` ``, ...) viram
/// `Char` também -- a palavra da gramática é só uma grafia alternativa
/// para o mesmo caractere lógico que `winit::keyboard::Key::Character`
/// entrega depois do layout aplicado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ChordKey {
    Char(char),
    Named(NamedKey),
}

/// Uma combinação resolvida -- a chave real do mapa de keybindings.
/// `Eq`/`Hash` por derive: como é um struct de flags, duas grafias de
/// texto diferentes que significam a mesma tecla produzem o mesmo
/// `Chord`, sem passo de canonicalização à parte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    ctrl: bool,
    alt: bool,
    shift: bool,
    cmd: bool,
    key: ChordKey,
}

/// Nomes de tecla que não são o próprio caractere -- a lista de
/// `docs/config/porecatu.example.toml` `[keybindings]`. Único array-fonte
/// entre `parse` (texto -> `ChordKey`) e a mensagem de erro (nomes
/// válidos para sugestão) -- as duas não podem divergir.
const NAMED_KEYS: &[(&str, ChordKey)] = &[
    ("equals", ChordKey::Char('=')),
    ("minus", ChordKey::Char('-')),
    ("comma", ChordKey::Char(',')),
    ("period", ChordKey::Char('.')),
    ("slash", ChordKey::Char('/')),
    ("backslash", ChordKey::Char('\\')),
    ("semicolon", ChordKey::Char(';')),
    ("quote", ChordKey::Char('\'')),
    ("bracketleft", ChordKey::Char('[')),
    ("bracketright", ChordKey::Char(']')),
    ("backtick", ChordKey::Char('`')),
    ("space", ChordKey::Named(NamedKey::Space)),
    ("tab", ChordKey::Named(NamedKey::Tab)),
    ("enter", ChordKey::Named(NamedKey::Enter)),
    ("escape", ChordKey::Named(NamedKey::Escape)),
    ("backspace", ChordKey::Named(NamedKey::Backspace)),
    ("delete", ChordKey::Named(NamedKey::Delete)),
    ("insert", ChordKey::Named(NamedKey::Insert)),
    ("home", ChordKey::Named(NamedKey::Home)),
    ("end", ChordKey::Named(NamedKey::End)),
    ("pageup", ChordKey::Named(NamedKey::PageUp)),
    ("pagedown", ChordKey::Named(NamedKey::PageDown)),
    ("up", ChordKey::Named(NamedKey::ArrowUp)),
    ("down", ChordKey::Named(NamedKey::ArrowDown)),
    ("left", ChordKey::Named(NamedKey::ArrowLeft)),
    ("right", ChordKey::Named(NamedKey::ArrowRight)),
];

fn named_f_key(text: &str) -> Option<ChordKey> {
    let n: u8 = text.strip_prefix('f')?.parse().ok()?;
    let named = match n {
        1 => NamedKey::F1,
        2 => NamedKey::F2,
        3 => NamedKey::F3,
        4 => NamedKey::F4,
        5 => NamedKey::F5,
        6 => NamedKey::F6,
        7 => NamedKey::F7,
        8 => NamedKey::F8,
        9 => NamedKey::F9,
        10 => NamedKey::F10,
        11 => NamedKey::F11,
        12 => NamedKey::F12,
        13 => NamedKey::F13,
        14 => NamedKey::F14,
        15 => NamedKey::F15,
        16 => NamedKey::F16,
        17 => NamedKey::F17,
        18 => NamedKey::F18,
        19 => NamedKey::F19,
        20 => NamedKey::F20,
        21 => NamedKey::F21,
        22 => NamedKey::F22,
        23 => NamedKey::F23,
        24 => NamedKey::F24,
        _ => return None,
    };
    Some(ChordKey::Named(named))
}

fn parse_key(text: &str) -> Option<ChordKey> {
    if let Some(&(_, key)) = NAMED_KEYS.iter().find(|(name, _)| *name == text) {
        return Some(key);
    }
    if let Some(key) = named_f_key(text) {
        return Some(key);
    }
    // Um caractere lógico só -- letra, dígito, ou um símbolo digitado
    // literalmente em vez da palavra da gramática (aceito por tolerância;
    // a gramática documentada usa a palavra).
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_none() {
        return Some(ChordKey::Char(c));
    }
    None
}

impl Chord {
    /// Parseia uma chave de `[keybindings]` (ADR-0029 §2):
    /// `modificador* tecla`, separados por `+`. Tolerante a maiúsculas --
    /// a gramática documentada é minúscula, mas rejeitar `Ctrl+T` só por
    /// causa do `C` maiúsculo não ajuda ninguém.
    pub fn parse(text: &str) -> Result<Chord, String> {
        let lower = text.to_lowercase();
        let mut parts: Vec<&str> = lower.split('+').collect();
        let Some(key_text) = parts.pop() else {
            return Err(format!("tecla vazia: \"{text}\""));
        };
        let mut chord = Chord {
            ctrl: false,
            alt: false,
            shift: false,
            cmd: false,
            key: ChordKey::Char('\0'),
        };
        for part in &parts {
            match *part {
                "ctrl" => chord.ctrl = true,
                "alt" => chord.alt = true,
                "shift" => chord.shift = true,
                "cmd" => chord.cmd = true,
                other => {
                    return Err(format!(
                        "modificador desconhecido: \"{other}\" em \"{text}\""
                    ));
                }
            }
        }
        let Some(key) = parse_key(key_text) else {
            return Err(format!("tecla desconhecida: \"{key_text}\" em \"{text}\""));
        };
        chord.key = key;
        Ok(chord)
    }

    /// A `Chord` de um evento de teclado de verdade, se a tecla lógica
    /// estiver no vocabulário da gramática -- `None` para o resto (uma
    /// tecla morta resolvida pelo SO em outro `Key`, uma tecla de mídia,
    /// composição de IME multi-caractere), que nunca casa binding nenhum
    /// e cai para o terminal como hoje.
    pub fn from_key(key: &Key, modifiers: Modifiers) -> Option<Chord> {
        let chord_key = match key {
            Key::Character(s) => {
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                ChordKey::Char(c.to_ascii_lowercase())
            }
            Key::Named(named) => ChordKey::Named(*named),
            _ => return None,
        };
        Some(Chord {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            cmd: modifiers.super_,
            key: chord_key,
        })
    }
}

/// Plataforma-alvo, injetada em vez de lida direto de `cfg!` -- é o que
/// torna a resolução dos três níveis testável para as três plataformas
/// na mesma máquina de CI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Windows
        }
    }
}

/// Aplica uma tabela (`[keybindings]`, `[keybindings.<plataforma>]`, ou
/// uma das duas tabelas embutidas) sobre `bindings`, na ordem do
/// ADR-0029 §3/§4:
///
/// - Duplicado (duas chaves de texto diferentes que resolvem pro mesmo
///   `Chord`, **dentro desta tabela**) é erro citando as duas grafias --
///   sem número de linha: a essa altura a config já virou
///   `BTreeMap<String, String>`, que não guarda posição no arquivo. Nenhuma
///   das duas linhas ambíguas desta tabela aplica; se uma camada anterior
///   (default embutido, ou a tabela comum) já tinha um valor pra esse
///   `Chord`, ele continua valendo -- só esta tabela é descartada, não o
///   mapa acumulado inteiro.
/// - Tecla malformada ou ação desconhecida descarta só aquela linha
///   (ADR-0029 §4) -- o resto da tabela aplica normalmente.
/// - `"none"` remove a entrada do mapa acumulado, mesmo que ela tenha
///   vindo de uma tabela anterior (é assim que o usuário libera um
///   default embutido).
fn apply_table(
    bindings: &mut HashMap<Chord, Action>,
    table: &BTreeMap<String, String>,
    issues: &mut Vec<String>,
) {
    // Primeiro passo: resolve cada chave de texto pra Chord, detectando
    // duplicado antes de tocar `bindings` -- um duplicado não deve
    // sobrescrever o valor anterior de `bindings` parcialmente.
    let mut by_chord: HashMap<Chord, Vec<&str>> = HashMap::new();
    let mut malformed = Vec::new();
    for key_text in table.keys() {
        match Chord::parse(key_text) {
            Ok(chord) => by_chord.entry(chord).or_default().push(key_text.as_str()),
            Err(msg) => malformed.push(msg),
        }
    }
    issues.extend(malformed);

    for (chord, key_texts) in by_chord {
        if key_texts.len() > 1 {
            issues.push(format!(
                "binding duplicado: {} resolvem pra mesma tecla",
                key_texts
                    .iter()
                    .map(|k| format!("\"{k}\""))
                    .collect::<Vec<_>>()
                    .join(" e ")
            ));
            continue;
        }
        let key_text = key_texts[0];
        let action_text = &table[key_text];
        if action_text == "none" {
            bindings.remove(&chord);
            continue;
        }
        match action_text.parse::<Action>() {
            Ok(action) => {
                bindings.insert(chord, action);
            }
            Err(err) => issues.push(format!("\"{key_text}\": {err}")),
        }
    }
}

/// Resultado de resolver os três níveis de `[keybindings]` (ADR-0029
/// §3): defaults embutidos da plataforma atual -> tabela comum do
/// usuário -> tabela da plataforma atual do usuário. Cada nível é
/// aplicado com `apply_table`, então um binding definido em dois níveis é
/// override, não duplicado -- só a mesma tabela duplicar a mesma tecla é
/// erro.
pub struct ResolvedKeymap {
    pub bindings: HashMap<Chord, Action>,
    /// Um item por chave malformada, ação desconhecida ou duplicado --
    /// vira aviso na superfície do ADR-0014 (RF-4.22-like, mas para
    /// `[keybindings]`), severidade aviso, persiste até dispensa.
    pub issues: Vec<String>,
}

pub fn resolve(user: &porecatu_config::Keybindings, platform: Platform) -> ResolvedKeymap {
    let embedded = porecatu_config::Keybindings::default();
    let mut bindings = HashMap::new();
    let mut issues = Vec::new();

    apply_table(&mut bindings, &embedded.common, &mut issues);
    if platform == Platform::Macos {
        apply_table(&mut bindings, &embedded.macos, &mut issues);
    }
    debug_assert!(
        issues.is_empty(),
        "defaults embutidos não deveriam produzir erro de keybinding: {issues:?}"
    );

    apply_table(&mut bindings, &user.common, &mut issues);
    let user_platform = match platform {
        Platform::Windows => &user.windows,
        Platform::Linux => &user.linux,
        Platform::Macos => &user.macos,
    };
    apply_table(&mut bindings, user_platform, &mut issues);

    ResolvedKeymap { bindings, issues }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Modifiers {
        Modifiers {
            ctrl,
            alt,
            shift,
            super_: cmd,
        }
    }

    #[test]
    fn shift_ctrl_and_ctrl_shift_canonicalize_to_the_same_chord() {
        assert_eq!(
            Chord::parse("shift+ctrl+t").unwrap(),
            Chord::parse("ctrl+shift+t").unwrap()
        );
    }

    #[test]
    fn uppercase_is_tolerated() {
        assert_eq!(
            Chord::parse("Ctrl+Shift+T").unwrap(),
            Chord::parse("ctrl+shift+t").unwrap()
        );
    }

    #[test]
    fn named_word_and_literal_symbol_are_the_same_key() {
        assert_eq!(
            Chord::parse("ctrl+comma").unwrap(),
            Chord::parse("ctrl+,").unwrap()
        );
    }

    #[test]
    fn unknown_modifier_is_an_error() {
        assert!(Chord::parse("super+t").is_err());
    }

    #[test]
    fn unknown_key_name_is_an_error() {
        assert!(Chord::parse("ctrl+bogus").is_err());
    }

    #[test]
    fn f_keys_parse_up_to_24() {
        assert!(Chord::parse("f24").is_ok());
        assert!(Chord::parse("f25").is_err());
        assert!(Chord::parse("f0").is_err());
    }

    #[test]
    fn from_key_matches_text_parse_for_named_key() {
        let from_text = Chord::parse("ctrl+shift+pagedown").unwrap();
        let from_event = Chord::from_key(
            &Key::Named(NamedKey::PageDown),
            mods(true, false, true, false),
        )
        .unwrap();
        assert_eq!(from_text, from_event);
    }

    #[test]
    fn from_key_matches_text_parse_for_character() {
        let from_text = Chord::parse("alt+1").unwrap();
        let from_event =
            Chord::from_key(&Key::Character("1".into()), mods(false, true, false, false)).unwrap();
        assert_eq!(from_text, from_event);
    }

    #[test]
    fn from_key_ignores_multi_char_ime_composition() {
        assert!(Chord::from_key(&Key::Character("ab".into()), Modifiers::NONE).is_none());
    }

    fn keybindings_with(common: &[(&str, &str)]) -> porecatu_config::Keybindings {
        porecatu_config::Keybindings {
            common: common
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            windows: BTreeMap::new(),
            linux: BTreeMap::new(),
            macos: BTreeMap::new(),
        }
    }

    #[test]
    fn embedded_defaults_resolve_without_issues_on_every_platform() {
        let empty = keybindings_with(&[]);
        for platform in [Platform::Windows, Platform::Linux, Platform::Macos] {
            let resolved = resolve(&empty, platform);
            assert!(
                resolved.issues.is_empty(),
                "{platform:?}: {:?}",
                resolved.issues
            );
        }
    }

    #[test]
    fn user_common_overrides_embedded_default() {
        let user = keybindings_with(&[("ctrl+shift+t", "window.new")]);
        let resolved = resolve(&user, Platform::Windows);
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        assert_eq!(resolved.bindings.get(&chord), Some(&Action::WindowNew));
    }

    #[test]
    fn none_releases_an_embedded_default() {
        let user = keybindings_with(&[("ctrl+shift+t", "none")]);
        let resolved = resolve(&user, Platform::Windows);
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        assert_eq!(resolved.bindings.get(&chord), None);
    }

    #[test]
    fn platform_table_overrides_common_table() {
        let mut user = keybindings_with(&[("ctrl+shift+t", "tab.new")]);
        user.macos
            .insert("ctrl+shift+t".to_owned(), "window.new".to_owned());
        let resolved_mac = resolve(&user, Platform::Macos);
        let resolved_win = resolve(&user, Platform::Windows);
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        assert_eq!(resolved_mac.bindings.get(&chord), Some(&Action::WindowNew));
        assert_eq!(resolved_win.bindings.get(&chord), Some(&Action::TabNew));
    }

    #[test]
    fn same_binding_in_common_and_platform_is_override_not_duplicate() {
        // Cobre a nota do ADR-0029 §3 explicitamente: mesma tecla na
        // tabela comum e na da plataforma não é duplicado.
        let mut user = keybindings_with(&[("ctrl+shift+g", "tab.new")]);
        user.windows
            .insert("ctrl+shift+g".to_owned(), "group.create".to_owned());
        let resolved = resolve(&user, Platform::Windows);
        assert!(resolved.issues.is_empty());
    }

    #[test]
    fn duplicate_in_the_same_table_is_an_issue_and_keeps_the_previous_layer() {
        let user = keybindings_with(&[
            ("ctrl+shift+t", "tab.close"),
            ("shift+ctrl+t", "window.new"),
        ]);
        let resolved = resolve(&user, Platform::Windows);
        assert_eq!(resolved.issues.len(), 1);
        assert!(resolved.issues[0].contains("duplicado"));
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        // Nenhuma das duas grafias ambíguas aplica -- mas o default
        // embutido (tab.new) já estava no mapa acumulado de uma camada
        // anterior, e um duplicado nesta camada não o desfaz: só as
        // linhas ambíguas desta tabela são descartadas.
        assert_eq!(resolved.bindings.get(&chord), Some(&Action::TabNew));
    }

    #[test]
    fn unknown_action_is_an_issue_with_suggestion() {
        let user = keybindings_with(&[("ctrl+shift+z", "tab.clsoe")]);
        let resolved = resolve(&user, Platform::Windows);
        assert_eq!(resolved.issues.len(), 1);
        assert!(resolved.issues[0].contains("tab.close"));
    }

    #[test]
    fn ctrl_tab_default_is_the_same_on_every_platform() {
        let empty = keybindings_with(&[]);
        for platform in [Platform::Windows, Platform::Linux, Platform::Macos] {
            let resolved = resolve(&empty, platform);
            let next = Chord::parse("ctrl+tab").unwrap();
            let prev = Chord::parse("ctrl+shift+tab").unwrap();
            assert_eq!(resolved.bindings.get(&next), Some(&Action::TabNext));
            assert_eq!(resolved.bindings.get(&prev), Some(&Action::TabPrev));
        }
    }

    #[test]
    fn app_quit_only_has_a_default_on_macos() {
        let empty = keybindings_with(&[]);
        let mac = resolve(&empty, Platform::Macos);
        let win = resolve(&empty, Platform::Windows);
        assert!(mac.bindings.values().any(|a| *a == Action::AppQuit));
        assert!(!win.bindings.values().any(|a| *a == Action::AppQuit));
    }
}
