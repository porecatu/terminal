// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz eventos de teclado/mouse do `winit` para `porecatu_term`
//! (ADR-0008, ADR-0013) -- `porecatu-term` não conhece GUI, então a
//! tradução mora aqui, não lá.
//!
//! Cadeia de resolução do ADR-0008 para teclado: (1) modo de captura --
//! não existe em F1, sem rename/busca ainda; (2) keybind de aplicação --
//! hoje só rolagem do scrollback (`Shift+PageUp`/`PageDown`, ADR-0013) e
//! copiar/colar (`Ctrl+Shift+C`/`V`, ADR-0008), os únicos defaults que não
//! dependem de `porecatu-config` (F4); (3) terminal -- codifica em bytes e
//! escreve no PTY.
//!
//! Para mouse, a regra de conflito do ADR-0013 é a mesma em todo lugar:
//! `Shift` força o comportamento local (seleção ou rolagem) sempre; sem
//! `Shift`, o programa ganha se tiver pedido o mouse; senão, local.

use std::time::{Duration, Instant};

use porecatu_term::{
    Modifiers, MouseAction, MouseButton as TermMouseButton, SelectionKind, SelectionSide, TermKey,
    TermModes, TermScroll, Terminal, encode_ctrl_char, encode_key, encode_mouse_report,
    encode_text, wrap_paste,
};
use winit::event::{ElementState, KeyEvent, MouseButton as WinitMouseButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::clipboard;
use crate::paint::CellMetrics;

/// Janela de tempo entre cliques no mesmo lugar para contar como duplo/
/// triplo clique. Sem procedência de design (não é valor de aparência) --
/// é o mesmo tipo de constante de interação que o dobro-clique do próprio
/// SO usa; 500ms é a faixa comum (é também o default do Windows).
const MULTI_CLICK_THRESHOLD: Duration = Duration::from_millis(500);

pub fn modifiers_from(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_: state.super_key(),
    }
}

fn named_term_key(named: NamedKey) -> Option<TermKey> {
    Some(match named {
        NamedKey::Enter => TermKey::Enter,
        NamedKey::Backspace => TermKey::Backspace,
        NamedKey::Tab => TermKey::Tab,
        NamedKey::Escape => TermKey::Escape,
        NamedKey::ArrowUp => TermKey::ArrowUp,
        NamedKey::ArrowDown => TermKey::ArrowDown,
        NamedKey::ArrowLeft => TermKey::ArrowLeft,
        NamedKey::ArrowRight => TermKey::ArrowRight,
        NamedKey::Home => TermKey::Home,
        NamedKey::End => TermKey::End,
        NamedKey::PageUp => TermKey::PageUp,
        NamedKey::PageDown => TermKey::PageDown,
        NamedKey::Insert => TermKey::Insert,
        NamedKey::Delete => TermKey::Delete,
        NamedKey::F1 => TermKey::Function(1),
        NamedKey::F2 => TermKey::Function(2),
        NamedKey::F3 => TermKey::Function(3),
        NamedKey::F4 => TermKey::Function(4),
        NamedKey::F5 => TermKey::Function(5),
        NamedKey::F6 => TermKey::Function(6),
        NamedKey::F7 => TermKey::Function(7),
        NamedKey::F8 => TermKey::Function(8),
        NamedKey::F9 => TermKey::Function(9),
        NamedKey::F10 => TermKey::Function(10),
        NamedKey::F11 => TermKey::Function(11),
        NamedKey::F12 => TermKey::Function(12),
        // Espaço, CapsLock, teclas de mídia etc.: sem codificação própria
        // de terminal -- cai para `event.text` (Espaço) ou é ignorada.
        _ => return None,
    })
}

/// Ponto de entrada do teclado. Eventos de IME (`Ime::Preedit`/`Commit`)
/// **não** passam por aqui -- vão direto pro terminal, sem consultar nada
/// (ADR-0008); ver `window_event` em `lib.rs`.
pub fn handle_keyboard_input(
    terminal: &Terminal,
    modes: &TermModes,
    event: &KeyEvent,
    modifiers: Modifiers,
    scroll_on_input: bool,
) {
    if event.state != ElementState::Pressed {
        return; // sem key-up reporting no v1
    }

    // `[terminal.scrollback] scroll_on_input` (RF-10.13): digitar volta ao
    // final, que é onde o prompt está. Só o passo 3 (bytes de verdade pro
    // programa) conta como "digitar" -- copiar/colar e a navegação do
    // scrollback (passo 2) não passam por aqui.
    let write = |bytes: Vec<u8>| {
        terminal.write(bytes);
        if scroll_on_input {
            terminal.scroll(TermScroll::Bottom);
        }
    };

    // Passo 2: keybind de aplicação.
    if modifiers.ctrl && modifiers.shift && !modifiers.alt {
        match &event.logical_key {
            // Copiar/colar (ADR-0008 default Windows/Linux: Ctrl+Shift+C/V).
            Key::Character(s) if s.eq_ignore_ascii_case("c") => {
                if let Some(text) = terminal.selection_text() {
                    clipboard::copy(&text);
                }
                return;
            }
            Key::Character(s) if s.eq_ignore_ascii_case("v") => {
                if let Some(text) = clipboard::paste() {
                    terminal.write(wrap_paste(&text, modes));
                }
                return;
            }
            _ => {}
        }
    }
    if modifiers.shift && !modifiers.ctrl && !modifiers.alt {
        match &event.logical_key {
            // Rolagem do scrollback (ADR-0013). Tela alternativa -> ação
            // não faz nada, mas ainda não cai pro terminal (um binding que
            // casa nunca cai, ADR-0008).
            Key::Named(NamedKey::PageUp) => {
                if !modes.alt_screen {
                    terminal.scroll(TermScroll::PageUp);
                }
                return;
            }
            Key::Named(NamedKey::PageDown) => {
                if !modes.alt_screen {
                    terminal.scroll(TermScroll::PageDown);
                }
                return;
            }
            // RF-11.30: `scrollback.to_top`/`to_bottom` já tinham default
            // embutido e já entravam no mapa resolvido -- faltava só isto.
            Key::Named(NamedKey::Home) => {
                if !modes.alt_screen {
                    terminal.scroll(TermScroll::Top);
                }
                return;
            }
            Key::Named(NamedKey::End) => {
                if !modes.alt_screen {
                    terminal.scroll(TermScroll::Bottom);
                }
                return;
            }
            _ => {}
        }
    }

    // Passo 3: terminal.
    if let Key::Named(named) = &event.logical_key
        && let Some(term_key) = named_term_key(*named)
    {
        write(encode_key(term_key, modifiers, modes));
        return;
    }

    if modifiers.ctrl
        && !modifiers.alt
        && let Key::Character(s) = &event.logical_key
        && let Some(c) = s.chars().next()
        && let Some(byte) = encode_ctrl_char(c)
    {
        write(vec![byte]);
        return;
    }

    if let Some(text) = &event.text {
        write(encode_text(text, modifiers));
    }
}

/// Roda do mouse: reporta ao programa se ele pediu o mouse (e `Shift` não
/// estiver forçando local); senão rola o scrollback, ou -- na tela
/// alternativa, sem scrollback -- vira setas quando `alternate_scroll`
/// estiver ligado (RF-10.14); desligado, a roda não faz nada ali (não há
/// scrollback pra rolar, e a tradução pra setas é o que ele desliga).
pub fn handle_mouse_wheel(
    terminal: &Terminal,
    modes: &TermModes,
    delta: MouseScrollDelta,
    modifiers: Modifiers,
    cell: CellPosition,
    scroll_multiplier: i32,
    alternate_scroll: bool,
) {
    let notches = match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        // Trackpad: sem notch discreto: 20px lógicos por linha é uma
        // aproximação de trabalho, sem procedência de design (não é valor
        // de aparência) -- ajustar se ficar ruim na prática.
        MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as f32,
    };
    if notches == 0.0 {
        return;
    }
    let lines = (notches.signum() as i32) * scroll_multiplier;

    if !modifiers.shift && modes.mouse_reporting != porecatu_term::MouseReporting::None {
        let button = if lines > 0 {
            TermMouseButton::WheelUp
        } else {
            TermMouseButton::WheelDown
        };
        let mut bytes = Vec::new();
        for _ in 0..lines.abs() {
            if let Some(report) = encode_mouse_report(
                button,
                MouseAction::Press,
                cell.col,
                cell.row,
                modifiers,
                modes,
            ) {
                bytes.extend(report);
            }
        }
        if !bytes.is_empty() {
            terminal.write(bytes);
        }
        return;
    }

    if modes.alt_screen {
        if !alternate_scroll {
            return;
        }
        let key = if lines > 0 {
            TermKey::ArrowUp
        } else {
            TermKey::ArrowDown
        };
        let mut bytes = Vec::new();
        for _ in 0..lines.abs() {
            bytes.extend(encode_key(key, Modifiers::NONE, modes));
        }
        terminal.write(bytes);
    } else {
        terminal.scroll(TermScroll::Lines(lines));
    }
}

/// Posição do cursor do mouse em célula (linha/coluna, 0-based) e de que
/// lado dela -- o que decide se a borda da seleção inclui a célula.
#[derive(Debug, Clone, Copy)]
pub struct CellPosition {
    pub row: usize,
    pub col: usize,
    pub side: SelectionSide,
}

/// Converte posição em pixels físicos pra célula, usando a métrica já
/// medida (Etapa 4). Fora da grade satura na borda -- arrastar a seleção
/// até fora da janela ainda estende até a última célula visível.
pub fn cell_at(x: f64, y: f64, metrics: CellMetrics, rows: usize, cols: usize) -> CellPosition {
    let col_f = (x as f32 / metrics.width).max(0.0);
    let row_f = (y as f32 / metrics.height).max(0.0);
    let col = (col_f as usize).min(cols.saturating_sub(1));
    let row = (row_f as usize).min(rows.saturating_sub(1));
    let side = if col_f.fract() < 0.5 {
        SelectionSide::Left
    } else {
        SelectionSide::Right
    };
    CellPosition { row, col, side }
}

fn term_button(button: WinitMouseButton) -> Option<TermMouseButton> {
    match button {
        WinitMouseButton::Left => Some(TermMouseButton::Left),
        WinitMouseButton::Middle => Some(TermMouseButton::Middle),
        WinitMouseButton::Right => Some(TermMouseButton::Right),
        _ => None,
    }
}

/// Conta cliques no mesmo lugar dentro de [`MULTI_CLICK_THRESHOLD`] para
/// decidir Simples/Semântica/Linha (RF-10.4). `winit` não dá isso pronto.
#[derive(Debug, Default)]
pub struct ClickTracker {
    last: Option<(Instant, usize, usize)>,
    count: u8,
}

impl ClickTracker {
    /// Registra um clique em `(row, col)` e devolve a contagem atual
    /// (1 = simples, 2 = duplo, 3 = triplo, e cicla de volta pra 1).
    fn register(&mut self, row: usize, col: usize) -> u8 {
        let now = Instant::now();
        let continues = self.last.is_some_and(|(t, r, c)| {
            r == row && c == col && now.duration_since(t) < MULTI_CLICK_THRESHOLD
        });
        self.count = if continues { (self.count % 3) + 1 } else { 1 };
        self.last = Some((now, row, col));
        self.count
    }
}

fn selection_kind(click_count: u8, alt: bool) -> SelectionKind {
    // Alt+arraste é retangular independente da contagem de clique
    // (ADR-0013 lista os quatro gestos como alternativas, não combináveis).
    if alt {
        return SelectionKind::Block;
    }
    match click_count {
        1 => SelectionKind::Simple,
        2 => SelectionKind::Semantic,
        _ => SelectionKind::Lines,
    }
}

/// Clique/soltar do mouse na área do terminal.
#[allow(clippy::too_many_arguments)]
pub fn handle_mouse_button(
    terminal: &Terminal,
    modes: &TermModes,
    button: WinitMouseButton,
    pressed: bool,
    cell: CellPosition,
    modifiers: Modifiers,
    click_tracker: &mut ClickTracker,
    copy_on_select: bool,
) {
    let Some(term_button) = term_button(button) else {
        return;
    };

    // Shift força local sempre; sem Shift, o programa ganha se pediu o
    // mouse (ADR-0013 -- é a regra que deixa copiar de dentro do htop).
    if !modifiers.shift && modes.mouse_reporting != porecatu_term::MouseReporting::None {
        let action = if pressed {
            MouseAction::Press
        } else {
            MouseAction::Release
        };
        if let Some(bytes) =
            encode_mouse_report(term_button, action, cell.col, cell.row, modifiers, modes)
        {
            terminal.write(bytes);
        }
        return;
    }

    // Seleção local: só botão esquerdo. Botão do meio (colar PRIMARY) não
    // é suportado no v1 (RF-10.9); botão direito fica pro menu de
    // contexto, que é F2+.
    if term_button != TermMouseButton::Left {
        return;
    }

    if pressed {
        let count = click_tracker.register(cell.row, cell.col);
        let kind = selection_kind(count, modifiers.alt);
        terminal.start_selection(kind, cell.row, cell.col, cell.side);
    } else if copy_on_select && let Some(text) = terminal.selection_text() {
        // RF-10.8: `copy_on_select` copia ao **soltar**, não durante o
        // arraste -- selecionar já é caro o bastante por frame sem também
        // escrever no clipboard a cada `CursorMoved`.
        clipboard::copy(&text);
    }
}

/// Movimento do mouse na área do terminal -- reporta ao programa (modo
/// 1002 com botão, 1003 mesmo sem) ou, localmente, estende a seleção em
/// andamento se o botão esquerdo estiver pressionado.
pub fn handle_mouse_motion(
    terminal: &Terminal,
    modes: &TermModes,
    cell: CellPosition,
    modifiers: Modifiers,
    button_down: Option<WinitMouseButton>,
) {
    if !modifiers.shift && modes.mouse_reporting != porecatu_term::MouseReporting::None {
        let button = button_down
            .and_then(term_button)
            .unwrap_or(TermMouseButton::None);
        if let Some(bytes) = encode_mouse_report(
            button,
            MouseAction::Motion,
            cell.col,
            cell.row,
            modifiers,
            modes,
        ) {
            terminal.write(bytes);
        }
        return;
    }

    if button_down == Some(WinitMouseButton::Left) {
        terminal.update_selection(cell.row, cell.col, cell.side);
    }
}
