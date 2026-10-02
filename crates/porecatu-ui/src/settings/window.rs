// SPDX-License-Identifier: GPL-3.0-or-later

//! `SettingsWindow`: a janela do SO da tela de configurações (ADR-0059 §1).
//! Guarda a própria `WindowSurface` -- o `GpuContext` e o atlas de glyphs são
//! os do processo, como para toda janela (ADR-0015) --, a geometria, o estado
//! de ponteiro e de teclado, o rascunho das alterações e o conteúdo medido do
//! grupo em vista; o desenho sai pela mesma pipeline de camadas das janelas de
//! terminal (ADR-0018), só que sem workspace do outro lado.
//!
//! Fora do macOS a janela não tem decoração nativa (ADR-0027): o cabeçalho
//! carrega o título, a drag region e os três botões de janela, com a mesma
//! geometria e o mesmo `resize_direction_at` da barra de abas. No macOS a
//! decoração é nativa e esta janela só desenha guia e painel.
//!
//! **A janela só produz edições.** Alterar um controle muda o rascunho
//! (`Draft`); gravar é do Salvar, que devolve `Press::Save` a quem possui a
//! janela -- ela não sabe onde o arquivo está, nem tem canal de avisos. Nada é
//! aplicado ao `Config` daqui: a recarga a quente é quem aplica (ADR-0058 §6).
//!
//! **Medir texto é caro.** O conteúdo do painel (texto cortado, larguras de
//! botão e de segmento) é medido por `content::build` uma vez e guardado; só
//! volta a ser medido quando a chave muda -- grupo, largura do painel, idioma,
//! config, ou uma mudança no rascunho. Nenhum caminho de pintura ou de hit-test
//! mede texto, salvo o do campo que está recebendo teclas.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use porecatu_config::{Config, Edit, EditValue};
use porecatu_core::Action;
use porecatu_locale::Catalog;
use porecatu_render::{
    Frame, GpuContext, Layer, Primitive, Quad, Rect, TextMeasurer, TextRun, WindowSurface, icon,
};
use porecatu_term::Modifiers;
use winit::dpi::PhysicalPosition;
use winit::event::{KeyEvent, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use super::catalog::{self, Control, OptionDef};
use super::choice_list::{self, ChoiceItem, ChoiceLayout, ChoiceList};
use super::content::{
    self, Block, ChipTone, Content, ContentKey, ControlView, RowView, ShortcutsView, ViewExtras,
    choice_label,
};
use super::draft::Draft;
use super::field_edit::{EditPart, Editing};
use super::file_state::{Banner, Disk, Effect, FileState};
use super::interact;
use super::layout::{
    self, BannerButton, BlockGeometry, ControlPart, Focus, FooterButton, Hit, Layout, Metrics,
    banner_geometry, focus_order_with_banner, footer_buttons, group_items, hit_test, next_focus,
};
use super::paint;
use super::save::Saved;
use super::shortcuts::{Capturing, Conflict, Shortcuts};
use super::{Group, HEADER_GAP_PX, PANEL_BACKGROUND, TITLE_SIZE_PX};
use crate::dialog::{ConfirmDialog, DialogButton};
use crate::input::modifiers_from;
use crate::keymap::{Capture, Chord, Platform};
use crate::messages::msg;
use crate::overlay::BODY_FONT;
use crate::palette::{ResolvedPalette, ResolvedTermPalette};
use crate::tab_bar::{self, TabBarStyle, WindowButtonHit};
use crate::text_field::apply_text_field_key;
use crate::tooltip::{Hover, HoverKey};
use crate::{
    DOUBLE_CLICK_THRESHOLD, access, bar_height, chrome, is_macos, language, overlay, titlebar,
};

/// O que a janela de configurações lê do processo, por chamada: o estilo da
/// barra (que ela reaproveita para o cabeçalho) e o `Config` em vigor.
#[derive(Clone, Copy)]
pub(crate) struct Env<'a> {
    pub style: &'a TabBarStyle,
    pub config: &'a Config,
}

/// O que um clique ou uma tecla pede a quem possui a janela. A janela não se
/// fecha, não abre arquivo nem grava sozinha: quem a guarda (`App`) é quem
/// sabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    Nothing,
    /// Pedido de fechar sem nada pendente: o botão de fechar do cabeçalho, o
    /// gesto do sistema ou `Esc`. Com pendências a janela abre o diálogo de
    /// três saídas (RF-16.4) e responde por [`Press::Answer`].
    Close,
    /// A resposta do diálogo de pendências.
    Answer(DialogAnswer),
    /// "Abrir arquivo no editor" (RF-16.14).
    OpenFile,
    /// Salvar, pelo botão ou por `Ctrl+S`/`Cmd+S` (RF-16.14). Só sai com
    /// pendência e nenhuma recusada; as edições estão em
    /// [`SettingsWindow::edits`].
    Save,
}

/// O que o usuário respondeu ao diálogo de pendências (RF-16.4). Cancelar
/// vale também para fechar a janela do sistema durante o diálogo, e para
/// "Salvar e fechar" com um valor recusado -- a tela volta para o usuário
/// corrigi-lo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogAnswer {
    Cancel,
    /// O rascunho já foi descartado; quem recebe conclui o fechamento.
    Discard,
    /// As edições estão em [`SettingsWindow::edits`]; quem recebe grava e,
    /// se deu certo, conclui o fechamento.
    Save,
}

/// O que o Salvar grava e sobre o quê.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavePlan {
    /// O texto que a tela viu do arquivo; `None` se ele não existia, e aí o
    /// Salvar parte do exemplo embutido (RF-16.21).
    pub base: Option<String>,
    pub edits: Vec<Edit>,
}

pub(crate) struct SettingsWindow {
    window: Arc<Window>,
    surface: WindowSurface,
    /// Pixels físicos por pixel lógico. Tudo aqui trabalha em lógico; só
    /// `WindowSurface` converte para físico (ADR-0018).
    scale: f32,
    logical_width: f32,
    logical_height: f32,
    /// Última posição do cursor, em pixels físicos -- `winit` só a entrega em
    /// `CursorMoved`, não em `MouseInput`.
    cursor_position: (f64, f64),
    /// Botão de janela sob o cursor, para o hover.
    hovered_button: Option<WindowButtonHit>,
    /// Instante do último clique na drag region, para o duplo clique que
    /// maximiza/restaura -- o mesmo padrão de `WindowState::
    /// last_titlebar_click`.
    last_titlebar_click: Option<Instant>,
    /// O mesmo `Arc<Catalog>` do processo (`App::catalog`); a troca de idioma
    /// o substitui aqui junto com as janelas de terminal.
    catalog: Arc<Catalog>,
    /// Adaptador de acessibilidade (ADR-0043 §1, ADR-0059 §5): um por janela,
    /// criado antes de ela ficar visível. Sem leitor de tela conectado, a
    /// árvore nem é montada (`update_if_active`).
    access_adapter: accesskit_winit::Adapter,
    /// O grupo escolhido na guia (RF-16.7). Começa no último escolhido na
    /// execução, que `App` guarda (RF-16.9).
    selected_group: Group,
    focus: Focus,
    /// Rolagem vertical do painel, em pixels lógicos de conteúdo.
    scroll: f32,
    /// Modificadores do teclado: `Shift+Tab`, `Ctrl+S`, e os do campo de
    /// texto.
    modifiers: Modifiers,
    /// O alvo sob o cursor: realce de item e de botão.
    hovered: Option<Hit>,
    /// Tooltip da descrição cortada e do botão de restaurar (ADR-0019,
    /// ADR-0060 §2).
    hover: Hover,
    /// O conteúdo medido do grupo em vista; refeito quando a chave muda.
    content: Option<Content>,
    /// Sobe a cada troca de catálogo, a cada recarga de config e a cada
    /// mudança no rascunho, e invalida o conteúdo guardado.
    generation: u64,
    /// O que o usuário mudou e ainda não gravou (RF-16.15).
    draft: Draft,
    /// O campo que está recebendo teclas, se algum.
    editing: Option<Editing>,
    /// O botão do mouse segue apertado dentro de um campo em edição: o
    /// arraste seleciona texto (ADR-0035).
    dragging_field: bool,
    /// A lista de um botão de escolha, aberta.
    choice: Option<ChoiceList>,
    /// Onde procurar arquivos de idioma, para a lista do idioma.
    locale_dirs: Vec<PathBuf>,
    /// O tema que `theme.cycle` pôs na sessão, se pôs: não é pendência, só a
    /// linha abaixo da lista de temas (RF-16.25).
    session_theme: Option<String>,
    /// O diálogo de pendências, aberto ao fechar com alterações (RF-16.4).
    /// Modal: enquanto existe, só ele recebe ponteiro e teclado.
    dialog: Option<ConfirmDialog>,
    /// O grupo Atalhos: a tabela da plataforma em edição (RF-16.31).
    shortcuts: Shortcuts,
    /// O texto do filtro do grupo Atalhos (RF-16.28); vale a cada tecla e
    /// vive enquanto a janela vive.
    filter: String,
    /// A captura de atalho em curso, se alguma (RF-16.29).
    capturing: Option<Capturing>,
    /// O que a tela sabe do arquivo: a base do Salvar e a faixa (RF-16.21 a
    /// RF-16.23).
    file: FileState,
}

impl SettingsWindow {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        window: Arc<Window>,
        surface: WindowSurface,
        scale: f32,
        catalog: Arc<Catalog>,
        access_adapter: accesskit_winit::Adapter,
        initial_group: Group,
        config: &Config,
        locale_dirs: Vec<PathBuf>,
        disk: &Disk,
    ) -> Self {
        let size = window.inner_size();
        Self {
            window,
            surface,
            scale,
            logical_width: size.width as f32 / scale,
            logical_height: size.height as f32 / scale,
            cursor_position: (0.0, 0.0),
            hovered_button: None,
            last_titlebar_click: None,
            catalog,
            access_adapter,
            selected_group: initial_group,
            focus: Focus::Sidebar,
            scroll: 0.0,
            modifiers: Modifiers::NONE,
            hovered: None,
            hover: Hover::default(),
            content: None,
            generation: 0,
            draft: Draft::new(config),
            editing: None,
            dragging_field: false,
            choice: None,
            locale_dirs,
            session_theme: None,
            dialog: None,
            shortcuts: Shortcuts::new(config, Platform::current()),
            filter: String::new(),
            capturing: None,
            file: FileState::new(disk),
        }
    }

    pub(crate) fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// O grupo escolhido, que `App` lembra até o fim da execução (RF-16.9).
    pub(crate) fn selected_group(&self) -> Group {
        self.selected_group
    }

    /// ADR-0043 §1: o adaptador precisa ver todo `WindowEvent` da janela,
    /// antes de ele ser tratado.
    pub(crate) fn process_access_event(&mut self, event: &WindowEvent) {
        self.access_adapter.process_event(&self.window, event);
    }

    /// RF-16.2: a janela já aberta vem para a frente em vez de abrir outra.
    /// Uma janela minimizada é restaurada antes -- `focus_window` sozinho não
    /// a tira da barra de tarefas.
    pub(crate) fn bring_to_front(&self) {
        if self.window.is_minimized() == Some(true) {
            self.window.set_minimized(false);
        }
        self.window.focus_window();
    }

    /// Há alteração não gravada? É a pergunta que `App` faz antes de
    /// encerrar o processo (ADR-0059 §2). Um campo em edição com o texto
    /// mexido conta: é uma alteração que só falta confirmar.
    pub(crate) fn has_pending_changes(&self) -> bool {
        self.draft.is_dirty()
            || self.shortcuts.is_dirty()
            || self
                .editing
                .as_ref()
                .is_some_and(|editing| editing.part != EditPart::Filter && editing.changed())
    }

    /// Há alteração no rascunho ou nos atalhos, com nenhum campo em edição por
    /// confirmar: o que o diálogo de pendências conta.
    fn has_unsaved(&self) -> bool {
        self.draft.is_dirty() || self.shortcuts.is_dirty()
    }

    /// O Salvar tem o que gravar e nada que o bloqueie: há pendência, e
    /// nenhuma foi recusada (RF-16.18).
    pub(crate) fn can_save(&self) -> bool {
        self.file.allows_edit()
            && (self.draft.is_dirty() || self.shortcuts.is_dirty())
            && !self.draft.has_invalid()
    }

    /// As edições do rascunho, na ordem do catálogo: o que o Salvar grava.
    pub(crate) fn edits(&self) -> Vec<Edit> {
        let mut edits = self.draft.edits();
        edits.extend(self.shortcuts.edits());
        edits
    }

    /// O Salvar gravou: o arquivo agora diz `saved`, e nenhuma pendência
    /// sobra. A tela mostra isso na hora, sem esperar a recarga a quente.
    pub(crate) fn save_succeeded(&mut self, saved: &Saved) {
        self.editing = None;
        self.choice = None;
        self.dialog = None;
        self.capturing = None;
        // O texto gravado é a base de agora: a recarga que ele dispara chega
        // igual a ela e nunca é lida como conflito (ADR-0058 §3).
        self.file.saved(saved.text.clone());
        self.draft.commit(&saved.config);
        self.shortcuts.commit(&saved.config);
        self.invalidate();
    }

    /// O que o Salvar vai gravar e sobre qual texto -- ou `None` se não há o
    /// que gravar. Com a faixa de conflito à vista, Salvar equivale a Manter
    /// minhas alterações (RF-16.23): a base passa a ser o arquivo como está, e
    /// as pendências se aplicam por cima, chave a chave.
    pub(crate) fn prepare_save(&mut self) -> Option<SavePlan> {
        if !self.can_save() {
            return None;
        }
        self.keep_my_changes();
        let edits = self.edits();
        if edits.is_empty() {
            return None;
        }
        Some(SavePlan {
            base: self.file.base().map(str::to_owned),
            edits,
        })
    }

    /// O disco foi lido de novo (uma recarga, ou um Salvar que achou o arquivo
    /// mudado): a decisão é por conteúdo (ADR-0059 §4) -- a própria gravação
    /// só atualiza o que se mostra, sem pendências a base troca em silêncio, e
    /// com pendências sobe a faixa de conflito. Arquivo inválido põe a tela
    /// em somente leitura até um texto válido (RF-16.22).
    pub(crate) fn observe_disk(&mut self, disk: &Disk) {
        let pending = self.has_pending_changes();
        match self.file.observe(disk, pending) {
            Effect::Show(config) => {
                self.draft.rebase(&config);
                // A linha em captura não muda com a recarga (ADR-0059 §3): ela
                // guarda os atalhos que tinha ao começar.
                self.shortcuts.rebase(&config);
            }
            Effect::Conflict => {}
            Effect::ReadOnly => {
                self.editing = None;
                self.choice = None;
                self.capturing = None;
                self.dragging_field = false;
            }
        }
        self.invalidate();
    }

    /// Manter minhas alterações: o arquivo como está vira a base, e as
    /// pendências ficam (as que o arquivo novo já tem somem).
    fn keep_my_changes(&mut self) {
        if let Some(config) = self.file.accept_disk() {
            self.draft.rebase(&config);
            self.shortcuts.rebase(&config);
            self.invalidate();
        }
    }

    /// Recarregar: o arquivo como está vira a base, e as pendências são
    /// descartadas.
    fn reload_from_disk(&mut self) {
        if let Some(config) = self.file.accept_disk() {
            self.editing = None;
            self.choice = None;
            self.capturing = None;
            self.draft.discard();
            self.shortcuts.discard();
            self.draft.rebase(&config);
            self.shortcuts.rebase(&config);
            self.invalidate();
        }
    }

    /// O tema que a sessão usa mudou (`theme.cycle`, restauração, ou o tema
    /// que sumiu do arquivo): a linha abaixo da lista de temas acompanha
    /// (RF-16.25). Sem mudança, nada é refeito.
    pub(crate) fn set_session_theme(&mut self, theme: Option<&str>) {
        if self.session_theme.as_deref() != theme {
            self.session_theme = theme.map(str::to_owned);
            self.invalidate();
        }
    }

    // ---- fechar com pendências (RF-16.4, RF-16.5)

    /// O usuário pediu para fechar -- botão do cabeçalho, gesto do sistema ou
    /// `Esc`. Sem pendências fecha; com elas abre o diálogo de três saídas, e
    /// com o diálogo já aberto o pedido conta como Cancelar.
    pub(crate) fn request_close(&mut self) -> Press {
        if self.dialog.is_some() {
            return self.answer(DialogAnswer::Cancel);
        }
        self.commit_edit();
        if self.has_unsaved() {
            self.open_dialog();
            Press::Nothing
        } else {
            Press::Close
        }
    }

    /// O app vai encerrar (a última janela de terminal fechou, ou `app.quit`):
    /// com pendências a janela vem para a frente com o diálogo e o
    /// encerramento espera pela resposta (RF-16.5). Devolve se há o que
    /// perguntar; sem pendências, quem chama encerra direto.
    pub(crate) fn ask_before_quit(&mut self) -> bool {
        self.commit_edit();
        if !self.has_unsaved() {
            return false;
        }
        self.bring_to_front();
        if self.dialog.is_none() {
            self.open_dialog();
        }
        true
    }

    fn open_dialog(&mut self) {
        self.choice = None;
        self.editing = None;
        self.dialog = Some(ConfirmDialog::settings_pending(
            &self.catalog,
            self.draft.pending_count() + self.shortcuts.pending_actions().len(),
        ));
        self.hover.dismiss();
        self.window.request_redraw();
    }

    /// Fecha o diálogo com a resposta `answer`. "Descartar e fechar" descarta
    /// o rascunho aqui mesmo; "Salvar e fechar" com um valor recusado volta à
    /// tela como Cancelar (nada a gravar enquanto houver recusa).
    fn answer(&mut self, answer: DialogAnswer) -> Press {
        self.dialog = None;
        self.window.request_redraw();
        match answer {
            DialogAnswer::Discard => {
                self.discard();
                Press::Answer(DialogAnswer::Discard)
            }
            DialogAnswer::Save if !self.can_save() => Press::Answer(DialogAnswer::Cancel),
            other => Press::Answer(other),
        }
    }

    /// A resposta que o botão `button` do diálogo dá.
    fn button_answer(button: DialogButton) -> DialogAnswer {
        match button {
            DialogButton::Cancel => DialogAnswer::Cancel,
            DialogButton::Discard => DialogAnswer::Discard,
            DialogButton::Confirm => DialogAnswer::Save,
        }
    }

    /// Onde o diálogo aberto fica. Mede o texto dos botões -- o diálogo é
    /// transitório, e a pintura dele sempre mediu, como a dos de terminal.
    fn dialog_layout(
        &self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
    ) -> Option<overlay::DialogLayout> {
        let dialog = self.dialog.as_ref()?;
        Some(overlay::layout_dialog(
            self.logical_width,
            self.logical_height,
            dialog,
            env.config,
            measurer,
        ))
    }

    /// Ponteiro com o diálogo aberto: o realce acompanha o botão sob o cursor.
    fn dialog_hover(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, point: (f32, f32)) {
        let Some(layout) = self.dialog_layout(env, measurer) else {
            return;
        };
        let hit = overlay::dialog_hit(&layout, point);
        if let Some(dialog) = &mut self.dialog
            && dialog.hovered() != hit
        {
            dialog.set_hovered(hit);
            self.window.request_redraw();
        }
        self.window.set_cursor(if hit.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        });
    }

    /// Clique com o diálogo aberto: um botão responde; fora dele, nada.
    fn dialog_click(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        point: (f32, f32),
    ) -> Press {
        let Some(layout) = self.dialog_layout(env, measurer) else {
            return Press::Nothing;
        };
        match overlay::dialog_hit(&layout, point) {
            Some(button) => self.answer(Self::button_answer(button)),
            None => Press::Nothing,
        }
    }

    /// Teclas com o diálogo aberto (ADR-0014): `Esc` cancela, `Enter` aciona o
    /// botão focado -- o Cancelar, de início --, e `Tab`/setas andam entre os
    /// três.
    fn dialog_key(&mut self, key: &Key) -> Press {
        let Some(dialog) = &mut self.dialog else {
            return Press::Nothing;
        };
        match key {
            Key::Named(NamedKey::Escape) => return self.answer(DialogAnswer::Cancel),
            Key::Named(NamedKey::Enter) => {
                let answer = Self::button_answer(dialog.focused());
                return self.answer(answer);
            }
            Key::Named(NamedKey::Tab) => {
                dialog.step_focus(if self.modifiers.shift { -1 } else { 1 });
            }
            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown) => dialog.step_focus(1),
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp) => dialog.step_focus(-1),
            _ => return Press::Nothing,
        }
        self.window.request_redraw();
        Press::Nothing
    }

    /// Troca de idioma ao vivo (ADR-0056 §9): o catálogo novo vale no próximo
    /// quadro, e o título da janela do SO -- que não é desenhado, é do
    /// sistema -- acompanha agora. O conteúdo medido é refeito, porque o texto
    /// mudou de largura, e a árvore de acessibilidade na próxima volta do
    /// event loop.
    pub(crate) fn set_catalog(&mut self, catalog: &Arc<Catalog>) {
        self.catalog = Arc::clone(catalog);
        self.window
            .set_title(&msg::settings::window_title(&self.catalog));
        // A lista aberta tem os rótulos cortados no idioma antigo.
        self.choice = None;
        self.invalidate();
    }

    /// O conteúdo mostrado mudou -- config, idioma, ou rascunho --, e o
    /// medido é refeito no próximo quadro.
    pub(crate) fn invalidate(&mut self) {
        self.generation += 1;
        // Um botão de rodapé que deixou de estar disponível não segura foco, e
        // nem um da faixa que já não existe.
        if let Focus::Footer(button) = self.focus
            && !self.available_buttons().contains(&button)
        {
            self.focus = Focus::Sidebar;
        }
        if let Focus::Banner(button) = self.focus
            && !self.banner_kinds().contains(&button)
        {
            self.focus = Focus::Sidebar;
        }
        self.window.request_redraw();
    }

    pub(crate) fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Acompanha o tamanho físico novo da janela.
    pub(crate) fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32) {
        self.surface.resize(gpu, width, height, self.scale);
        self.logical_width = width as f32 / self.scale;
        self.logical_height = height as f32 / self.scale;
        self.choice = None;
        self.window.request_redraw();
    }

    /// A janela foi para um monitor de outra escala: o tamanho físico
    /// continua o que o SO deu, o lógico é que muda.
    pub(crate) fn rescale(&mut self, gpu: &GpuContext, scale: f32) {
        self.scale = scale;
        let size = self.window.inner_size();
        self.resize(gpu, size.width, size.height);
    }

    // ---- geometria

    /// Altura do cabeçalho: a da barra de abas fora do macOS (ADR-0060 §1),
    /// zero onde a decoração é nativa.
    fn header_height(style: &TabBarStyle) -> f32 {
        if is_macos() { 0.0 } else { bar_height(style) }
    }

    fn metrics(&self, env: Env<'_>) -> Metrics {
        Metrics::from_config(env.config, Self::header_height(env.style))
    }

    fn layout(&self, env: Env<'_>) -> Layout {
        let m = self.metrics(env);
        let banner_height = self.file.banner().map_or(0.0, |banner| {
            m.banner_height(matches!(banner, Banner::Invalid(_)))
        });
        layout::layout_with_banner(
            self.logical_width,
            self.logical_height,
            m.header_height,
            m.sidebar_width,
            m.footer_height(),
            banner_height,
        )
    }

    /// Os botões que a faixa do estado do arquivo oferece, na ordem.
    fn banner_kinds(&self) -> Vec<BannerButton> {
        match self.file.banner() {
            None => Vec::new(),
            Some(Banner::Conflict) => vec![BannerButton::Reload, BannerButton::Keep],
            Some(Banner::Invalid(_)) => vec![BannerButton::OpenFile],
        }
    }

    /// Os retângulos dos botões da faixa, em coordenadas de janela.
    fn banner_buttons(&self, env: Env<'_>) -> Vec<(BannerButton, Rect)> {
        let (Some(banner), Some(rect)) = (
            self.content.as_ref().and_then(|c| c.banner.as_ref()),
            self.layout(env).banner,
        ) else {
            return Vec::new();
        };
        let widths: Vec<f32> = banner.buttons.iter().map(|b| b.width).collect();
        let geometry = banner_geometry(&self.metrics(env), rect, &widths, banner.body.is_some());
        banner
            .buttons
            .iter()
            .zip(geometry.buttons)
            .map(|(view, rect)| (view.button, rect))
            .collect()
    }

    /// Um clique (ou `Enter`) num botão da faixa (RF-16.22, RF-16.23).
    fn press_banner(&mut self, button: BannerButton) -> Press {
        self.focus = Focus::Banner(button);
        match button {
            BannerButton::Reload => self.reload_from_disk(),
            BannerButton::Keep => self.keep_my_changes(),
            BannerButton::OpenFile => return Press::OpenFile,
        }
        self.window.request_redraw();
        Press::Nothing
    }

    fn cursor_logical(&self) -> (f32, f32) {
        (
            self.cursor_position.0 as f32 / self.scale,
            self.cursor_position.1 as f32 / self.scale,
        )
    }

    /// Garante o conteúdo do grupo em vista e a rolagem dentro do possível.
    /// É o único ponto que mede texto.
    fn ensure_content(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) {
        let m = self.metrics(env);
        let layout = self.layout(env);
        let key = ContentKey {
            group: self.selected_group,
            panel_width_bits: layout.panel.width.to_bits(),
            generation: self.generation,
        };
        if self.content.as_ref().map(|c| c.key) != Some(key) {
            let view = ShortcutsView {
                state: &self.shortcuts,
                filter: &self.filter,
                capturing: self.capturing.as_ref(),
            };
            let banner = self.file.banner();
            let extras = ViewExtras {
                session_theme: self.session_theme.as_deref(),
                shortcuts: Some(&view),
                banner: banner.as_ref(),
            };
            self.content = Some(content::build(
                key,
                &self.draft,
                &extras,
                &self.catalog,
                &m,
                measurer,
            ));
        }
        if let Some(content) = &self.content {
            self.scroll = layout::clamp_scroll(
                self.scroll,
                content.geometry.content_height,
                layout.panel_body.height,
            );
        }
    }

    fn content(&self) -> &Content {
        self.content
            .as_ref()
            .expect("ensure_content roda antes de qualquer leitura")
    }

    /// A opção que a linha `block` edita, se ela é de opção.
    fn option_at(&self, block: usize) -> Option<&'static OptionDef> {
        let id = self.content.as_ref()?.row(block)?.option?;
        catalog::option(id)
    }

    /// Quais botões do rodapé estão disponíveis, na ordem de
    /// `FOOTER_BUTTONS`: Abrir arquivo sempre; Descartar com pendência;
    /// Salvar com pendência e nenhuma recusada (RF-16.14, RF-16.18).
    fn available_footer(&self) -> [bool; 3] {
        let pending = self.has_pending_changes() && self.file.allows_edit();
        [true, pending, pending && !self.draft.has_invalid()]
    }

    fn available_buttons(&self) -> Vec<FooterButton> {
        layout::FOOTER_BUTTONS
            .iter()
            .zip(self.available_footer())
            .filter(|(_, available)| *available)
            .map(|(button, _)| *button)
            .collect()
    }

    /// O alvo sob `point`: guia, rodapé, ou -- numa linha do painel -- o botão
    /// de restaurar, uma parte do controle, ou o fundo da linha.
    fn hit_at(&self, env: Env<'_>, point: (f32, f32)) -> Option<Hit> {
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);
        if let Some((button, _)) = self
            .banner_buttons(env)
            .into_iter()
            .find(|(_, rect)| tab_bar::rect_contains(*rect, point))
        {
            return Some(Hit::Banner(button));
        }
        let hit = hit_test(
            &layout,
            &items,
            &footer,
            &content.geometry,
            self.scroll,
            point,
        );
        match hit {
            Some(Hit::Row(index)) => Some(self.refine_row_hit(env, index, point)),
            other => other,
        }
    }

    /// Dentro de uma linha: o botão de restaurar vence o controle, que vence o
    /// fundo.
    fn refine_row_hit(&self, env: Env<'_>, index: usize, point: (f32, f32)) -> Hit {
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        let (Some(row), Some(BlockGeometry::Row(geometry))) =
            (content.row(index), content.geometry.blocks.get(index))
        else {
            return Hit::Row(index);
        };
        let (dx, dy) = (layout.panel_body.x, layout.panel_body.y - self.scroll);
        let shift = |rect: Rect| Rect {
            x: rect.x + dx,
            y: rect.y + dy,
            ..rect
        };
        if row.can_reset && tab_bar::rect_contains(shift(geometry.restore), point) {
            return Hit::Restore(index);
        }
        match row.control.part_at(shift(geometry.control), &m, point) {
            Some(part) => Hit::Control(index, part),
            None => Hit::Row(index),
        }
    }

    /// O retângulo de um controle da linha `block`, em coordenadas de janela.
    fn control_rect(&self, env: Env<'_>, block: usize) -> Option<Rect> {
        let layout = self.layout(env);
        let BlockGeometry::Row(geometry) = self.content().geometry.blocks.get(block)? else {
            return None;
        };
        Some(Rect {
            x: geometry.control.x + layout.panel_body.x,
            y: geometry.control.y + layout.panel_body.y - self.scroll,
            ..geometry.control
        })
    }

    /// O campo que recebe o texto de `part`, em coordenadas de janela: o
    /// controle inteiro, ou -- no Git -- o número, à direita da alternância.
    fn field_rect(&self, env: Env<'_>, block: usize, part: EditPart) -> Option<Rect> {
        if part == EditPart::Filter {
            let layout = self.layout(env);
            let BlockGeometry::Filter { rect } = self.content().geometry.blocks.get(block)? else {
                return None;
            };
            return Some(Rect {
                x: rect.x + layout.panel_body.x,
                y: rect.y + layout.panel_body.y - self.scroll,
                ..*rect
            });
        }
        let control = self.control_rect(env, block)?;
        Some(match part {
            EditPart::Field => control,
            EditPart::GitSeconds => {
                let width = self.metrics(env).number_field_width;
                Rect {
                    x: control.x + control.width - width,
                    width,
                    ..control
                }
            }
            EditPart::Filter => return None,
            EditPart::ListFirst(item) | EditPart::ListSecond(item) => {
                let ControlView::List {
                    items, two_fields, ..
                } = &self.content().row(block)?.control
                else {
                    return None;
                };
                let geometry =
                    layout::list_geometry(control, &self.metrics(env), items.len(), *two_fields);
                let rects = geometry.items.get(item)?;
                if matches!(part, EditPart::ListSecond(_)) {
                    rects.second?
                } else {
                    rects.first
                }
            }
        })
    }

    // ---- ponteiro

    /// Borda de resize sob o ponto, se alguma: a mesma regra das janelas de
    /// terminal (ADR-0027) -- o botão de janela vence a borda no canto, e a
    /// janela maximizada não tem borda. Nunca no macOS (decoração nativa).
    fn resize_direction(
        &self,
        style: &TabBarStyle,
        resize_border: f32,
        point: (f32, f32),
    ) -> Option<winit::window::ResizeDirection> {
        if is_macos() || self.window.is_maximized() {
            return None;
        }
        let over_button = tab_bar::point_in_window_button(
            style,
            false,
            self.logical_width,
            bar_height(style),
            point,
        )
        .is_some();
        if over_button {
            return None;
        }
        titlebar::resize_direction_at(
            point,
            self.logical_width,
            self.logical_height,
            false,
            resize_border,
        )
    }

    /// Movimento do cursor: guarda a posição, escolhe a forma do cursor (setas
    /// de resize nas bordas, mão sobre o que se clica, I sobre campo de
    /// texto), atualiza o realce e o tooltip, arrasta a seleção de um campo em
    /// edição, e pede quadro só se algo visível mudou.
    pub(crate) fn cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        now: Instant,
    ) {
        self.cursor_position = (position.x, position.y);
        self.ensure_content(env, measurer);
        let point = self.cursor_logical();
        let resize_border = env.config.appearance.window_controls.resize_border as f32;

        if self.dragging_field {
            self.drag_selection(env, measurer, point);
            return;
        }

        let hovered_button = if is_macos() {
            None
        } else {
            tab_bar::point_in_window_button(
                env.style,
                false,
                self.logical_width,
                bar_height(env.style),
                point,
            )
        };

        // O diálogo de pendências cobre a janela: só ele reage ao ponteiro.
        if self.dialog.is_some() {
            self.dialog_hover(env, measurer, point);
            return;
        }

        // A lista aberta cobre o resto: só ela reage ao ponteiro.
        if self.choice.is_some() {
            let highlighted_before = self.choice.as_ref().map(|list| list.highlighted);
            if let (Some(layout), Some(list)) = (self.choice_layout(env), self.choice.as_mut())
                && let Some(index) = choice_list::hit(&layout, point)
            {
                list.highlighted = index;
            }
            self.window.set_cursor(CursorIcon::Default);
            if highlighted_before != self.choice.as_ref().map(|list| list.highlighted) {
                self.window.request_redraw();
            }
            return;
        }

        let hit = self.hit_at(env, point);
        // Descartar e Salvar indisponíveis não realçam.
        let hit = match hit {
            Some(Hit::Footer(button)) if !self.available_buttons().contains(&button) => None,
            other => other,
        };
        let cursor = match self.resize_direction(env.style, resize_border, point) {
            Some(direction) => CursorIcon::from(direction),
            None => self.cursor_for(hit),
        };
        self.window.set_cursor(cursor);

        let before = (
            self.hovered,
            self.hovered_button,
            self.hover.visible().is_some(),
        );
        self.hovered = hit;
        self.hovered_button = hovered_button;
        let target = self.tooltip_target(env, point, hit);
        self.hover.update(target, now);
        let after = (
            self.hovered,
            self.hovered_button,
            self.hover.visible().is_some(),
        );
        if before != after {
            self.window.request_redraw();
        }
    }

    /// A forma do cursor sobre `hit`: I sobre campo de texto, mão sobre o que
    /// se clica.
    fn cursor_for(&self, hit: Option<Hit>) -> CursorIcon {
        match hit {
            Some(Hit::Group(_) | Hit::Footer(_) | Hit::Restore(_) | Hit::Banner(_)) => {
                CursorIcon::Pointer
            }
            Some(Hit::Control(index, part)) => {
                match self.content().row(index).map(|row| &row.control) {
                    Some(ControlView::Field { .. }) => CursorIcon::Text,
                    Some(ControlView::List { .. }) => match part {
                        ControlPart::ListField { .. } => CursorIcon::Text,
                        _ => CursorIcon::Pointer,
                    },
                    Some(ControlView::Themes { .. }) | None => CursorIcon::Default,
                    Some(_) => CursorIcon::Pointer,
                }
            }
            // A linha de um tema é toda alvo do clique (RF-16.24).
            Some(Hit::Row(index)) => {
                if matches!(
                    self.content().row(index).map(|row| &row.control),
                    Some(ControlView::Themes { .. })
                ) {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                }
            }
            None => CursorIcon::Default,
        }
    }

    /// O alvo do tooltip: o botão de restaurar sob o cursor (um ícone sem
    /// texto) ou a descrição de uma linha cujo texto foi cortado. Descrição que
    /// cabe inteira não tem tooltip (ADR-0019).
    fn tooltip_target(
        &self,
        env: Env<'_>,
        point: (f32, f32),
        hit: Option<Hit>,
    ) -> Option<(HoverKey, Rect, String)> {
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        match hit? {
            Hit::Restore(index) => {
                let BlockGeometry::Row(geometry) = content.geometry.blocks.get(index)? else {
                    return None;
                };
                let area = Rect {
                    x: geometry.restore.x + layout.panel_body.x,
                    y: geometry.restore.y + layout.panel_body.y - self.scroll,
                    ..geometry.restore
                };
                Some((
                    HoverKey::SettingsRestore(index),
                    area,
                    msg::settings::button::restore_default(&self.catalog),
                ))
            }
            Hit::Row(index) => {
                let row = content.row(index)?;
                if !row.description_truncated {
                    return None;
                }
                let BlockGeometry::Row(geometry) = content.geometry.blocks.get(index)? else {
                    return None;
                };
                let area = Rect {
                    x: geometry.description_origin.0 + layout.panel_body.x,
                    y: geometry.description_origin.1 + layout.panel_body.y - self.scroll,
                    width: geometry.left_width,
                    height: m.description_size,
                };
                tab_bar::rect_contains(area, point).then(|| {
                    (
                        HoverKey::SettingsRow(index),
                        area,
                        row.description_full.clone(),
                    )
                })
            }
            _ => None,
        }
    }

    /// O cursor saiu da janela: somem os realces e o tooltip.
    pub(crate) fn cursor_left(&mut self) {
        let changed = self.hovered.take().is_some()
            || self.hovered_button.take().is_some()
            || self.hover.visible().is_some();
        self.hover.dismiss();
        if changed {
            self.window.request_redraw();
        }
    }

    /// Roda do mouse sobre o painel: rola na vertical, limitada ao fim do
    /// conteúdo (RF-16.8).
    pub(crate) fn wheel(
        &mut self,
        delta: MouseScrollDelta,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
    ) {
        self.ensure_content(env, measurer);
        let layout = self.layout(env);
        if !tab_bar::rect_contains(layout.panel_body, self.cursor_logical()) {
            return;
        }
        let pixels = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines * env.style.overflow_scroll_step,
            MouseScrollDelta::PixelDelta(position) => position.y as f32 / self.scale,
        };
        let content_height = self.content().geometry.content_height;
        self.scroll = layout::clamp_scroll(
            self.scroll - pixels,
            content_height,
            layout.panel_body.height,
        );
        // A lista é ancorada à linha: rolar a deixaria solta.
        self.choice = None;
        self.hover.dismiss();
        self.window.request_redraw();
    }

    /// Clique esquerdo (ADR-0027): botão de janela, borda de resize ou drag
    /// region do cabeçalho; depois o que o painel tem -- um grupo da guia, o
    /// rodapé, o controle de uma linha.
    pub(crate) fn left_pressed(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) -> Press {
        self.ensure_content(env, measurer);
        self.hover.dismiss();
        let style = env.style;
        let resize_border = env.config.appearance.window_controls.resize_border as f32;
        let point = self.cursor_logical();
        if !is_macos() {
            if let Some(hit) = tab_bar::point_in_window_button(
                style,
                false,
                self.logical_width,
                bar_height(style),
                point,
            ) {
                return match hit {
                    WindowButtonHit::Minimize => {
                        self.window.set_minimized(true);
                        Press::Nothing
                    }
                    WindowButtonHit::MaximizeRestore => {
                        self.window.set_maximized(!self.window.is_maximized());
                        Press::Nothing
                    }
                    WindowButtonHit::Close => self.request_close(),
                };
            }
            if let Some(direction) = self.resize_direction(style, resize_border, point) {
                let _ = self.window.drag_resize_window(direction);
                return Press::Nothing;
            }
            if point.1 < Self::header_height(style) {
                self.resolve_titlebar_drag();
                return Press::Nothing;
            }
        }
        // O diálogo de pendências cobre a janela inteira: só ele recebe o
        // clique.
        if self.dialog.is_some() {
            return self.dialog_click(env, measurer, point);
        }
        // A lista aberta é modal: um clique escolhe um item ou a fecha, e em
        // nenhum dos dois casos chega ao que está por baixo.
        if self.choice.is_some() {
            self.click_choice(env, point);
            return Press::Nothing;
        }
        let hit = self.hit_at(env, point);
        // Um clique fora do chip e dos botões do conflito cancela a captura.
        if self.capturing.is_some()
            && !matches!(
                hit,
                Some(Hit::Control(
                    _,
                    ControlPart::Chip(_) | ControlPart::Segment(_)
                ))
            )
        {
            self.capturing = None;
            self.invalidate();
        }
        match hit {
            Some(Hit::Group(group)) => {
                self.focus = Focus::Sidebar;
                self.select_group(group);
                Press::Nothing
            }
            Some(Hit::Footer(button)) => self.press_footer(button),
            Some(Hit::Banner(button)) => {
                self.commit_edit();
                self.press_banner(button)
            }
            // Somente leitura (RF-16.22): o clique só leva o foco à linha.
            Some(Hit::Restore(index) | Hit::Control(index, _)) if !self.file.allows_edit() => {
                self.commit_edit();
                self.focus = Focus::Row(index);
                self.window.request_redraw();
                Press::Nothing
            }
            Some(Hit::Restore(index)) => {
                self.commit_edit();
                self.focus = Focus::Row(index);
                if let Some(action) = self.content().row(index).and_then(|row| row.action) {
                    self.shortcuts.reset(action);
                } else if let Some(option) = self.option_at(index) {
                    self.draft.reset(option);
                }
                self.invalidate();
                Press::Nothing
            }
            Some(Hit::Control(index, part)) => {
                self.click_control(env, measurer, index, part, point);
                Press::Nothing
            }
            Some(Hit::Row(index)) => {
                // O campo de filtro: o clique começa a digitar.
                if matches!(self.content().blocks.get(index), Some(Block::Filter { .. })) {
                    self.focus = Focus::Row(index);
                    self.begin_or_continue_edit(env, measurer, index, EditPart::Filter, point);
                    self.window.request_redraw();
                    return Press::Nothing;
                }
                self.commit_edit();
                self.focus = Focus::Row(index);
                if !(self.file.allows_edit() && self.choose_theme(index)) {
                    self.window.request_redraw();
                }
                Press::Nothing
            }
            None => {
                self.commit_edit();
                Press::Nothing
            }
        }
    }

    /// O botão esquerdo foi solto: acaba o arraste de seleção de um campo.
    pub(crate) fn left_released(&mut self) {
        self.dragging_field = false;
    }

    /// Clique num botão do rodapé. Descartar e Salvar indisponíveis não fazem
    /// nada. Salvar confirma antes o campo em edição -- o que está digitado
    /// entra no que se grava -- e só sai se nada ficou recusado.
    fn press_footer(&mut self, button: FooterButton) -> Press {
        match button {
            FooterButton::OpenFile => {
                self.focus = Focus::Footer(FooterButton::OpenFile);
                self.window.request_redraw();
                Press::OpenFile
            }
            FooterButton::Discard if self.available_buttons().contains(&button) => {
                self.focus = Focus::Footer(button);
                self.discard();
                Press::Nothing
            }
            FooterButton::Save if self.available_buttons().contains(&button) => {
                self.focus = Focus::Footer(button);
                self.commit_edit();
                if self.can_save() {
                    Press::Save
                } else {
                    Press::Nothing
                }
            }
            FooterButton::Discard | FooterButton::Save => Press::Nothing,
        }
    }

    /// Descartar (RF-16.15): nenhuma pendência, e a tela volta a mostrar o que
    /// o arquivo diz.
    fn discard(&mut self) {
        self.editing = None;
        self.choice = None;
        self.dragging_field = false;
        self.capturing = None;
        self.draft.discard();
        self.shortcuts.discard();
        self.invalidate();
    }

    /// Clique numa parte do controle da linha `index`.
    fn click_control(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        index: usize,
        part: ControlPart,
        point: (f32, f32),
    ) {
        self.focus = Focus::Row(index);
        // Uma linha de atalho: o chip entra em captura, e os dois botões do
        // conflito respondem (RF-16.29, RF-16.30).
        if let Some(action) = self.content().row(index).and_then(|row| row.action) {
            self.commit_edit();
            match part {
                ControlPart::Chip(chip) => self.click_chip(index, chip, action),
                ControlPart::Segment(button) => self.answer_conflict(button == 0),
                _ => {}
            }
            self.window.request_redraw();
            return;
        }
        let Some(option) = self.option_at(index) else {
            self.commit_edit();
            return;
        };
        let control = self.content().row(index).map(|row| row.control.clone());
        match (control, part) {
            (Some(ControlView::Toggle { .. }), _) => {
                self.commit_edit();
                self.toggle(option);
            }
            (Some(ControlView::Segmented { .. }), ControlPart::Segment(segment)) => {
                self.commit_edit();
                self.choose_segment(option, segment);
            }
            (Some(ControlView::Field { .. }), _) => {
                self.begin_or_continue_edit(env, measurer, index, EditPart::Field, point);
            }
            (Some(ControlView::Choice { .. }), _) => {
                self.commit_edit();
                self.open_choice(env, measurer, index);
            }
            (Some(ControlView::GitPoll { .. }), ControlPart::GitToggle) => {
                self.commit_edit();
                self.toggle_git(option);
            }
            (Some(ControlView::GitPoll { on: true, .. }), ControlPart::GitNumber) => {
                self.begin_or_continue_edit(env, measurer, index, EditPart::GitSeconds, point);
            }
            (Some(ControlView::List { .. }), ControlPart::ListField { item, second }) => {
                let part = if second {
                    EditPart::ListSecond(item)
                } else {
                    EditPart::ListFirst(item)
                };
                self.begin_or_continue_edit(env, measurer, index, part, point);
            }
            (Some(ControlView::List { .. }), ControlPart::ListRemove(item)) => {
                self.commit_edit();
                if interact::list_remove(&mut self.draft, option, item) {
                    self.invalidate();
                }
            }
            (Some(ControlView::List { .. }), ControlPart::ListAdd) => {
                self.commit_edit();
                self.add_list_item(index, option);
            }
            _ => self.commit_edit(),
        }
        self.window.request_redraw();
    }

    /// Acrescenta um item vazio ao fim da lista da linha `index` e põe o
    /// primeiro campo dele em edição. O conteúdo medido ainda é o antigo --
    /// a edição nasce direto, sem lê-lo.
    fn add_list_item(&mut self, index: usize, option: &OptionDef) {
        if let Some(item) = interact::list_add(&mut self.draft, option) {
            self.invalidate();
            self.editing = Some(Editing::new(
                index,
                EditPart::ListFirst(item),
                String::new(),
            ));
        }
    }

    /// Escolhe o tema da linha `index`, se a linha é de tema (RF-16.24): a
    /// linha inteira é o alvo do clique.
    fn choose_theme(&mut self, index: usize) -> bool {
        let Some(ControlView::Themes { name, .. }) =
            self.content().row(index).map(|row| row.control.clone())
        else {
            return false;
        };
        let Some(option) = self.option_at(index) else {
            return false;
        };
        interact::choose_value(&mut self.draft, option, &name);
        self.invalidate();
        true
    }

    /// Clique na lista aberta: um item a escolhe; fora do menu a fecha.
    fn click_choice(&mut self, env: Env<'_>, point: (f32, f32)) {
        let Some(layout) = self.choice_layout(env) else {
            self.choice = None;
            return;
        };
        match choice_list::hit(&layout, point) {
            Some(index) => {
                if let Some(list) = &mut self.choice {
                    list.highlighted = index;
                }
                self.select_choice();
            }
            None if choice_list::contains(&layout, point) => {}
            None => {
                self.choice = None;
                self.window.request_redraw();
            }
        }
    }

    /// Clique na parte de um campo: se o campo já é o que está em edição, só
    /// move o cursor (e arma o arraste); senão confirma o outro e começa a
    /// editar este, com o cursor onde se clicou.
    fn begin_or_continue_edit(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        index: usize,
        part: EditPart,
        point: (f32, f32),
    ) {
        let already = self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.block == index && editing.part == part);
        if !already {
            self.commit_edit();
            let Some(initial) = self.edit_initial(index, part) else {
                return;
            };
            self.editing = Some(Editing::new(index, part, initial));
        }
        if let Some(byte) = self.byte_at(env, measurer, point)
            && let Some(editing) = &mut self.editing
        {
            editing.state.click_at(byte);
            self.dragging_field = true;
        }
    }

    /// O texto com que a edição de `part` na linha `index` começa: o que o
    /// campo mostra (escrito, se a opção escreve os controles; o digitado, se
    /// foi recusado).
    fn edit_initial(&self, index: usize, part: EditPart) -> Option<String> {
        if part == EditPart::Filter {
            return Some(self.filter.clone());
        }
        match (&self.content().row(index)?.control, part) {
            (ControlView::Field { text, .. }, EditPart::Field) => Some(text.clone()),
            (ControlView::GitPoll { seconds, .. }, EditPart::GitSeconds) => Some(seconds.clone()),
            (ControlView::List { items, .. }, EditPart::ListFirst(item)) => {
                items.get(item).map(|item| item.first.clone())
            }
            (ControlView::List { items, .. }, EditPart::ListSecond(item)) => {
                items.get(item).map(|item| item.second.clone())
            }
            _ => None,
        }
    }

    /// A posição, em bytes, do texto do campo em edição que o ponteiro
    /// aponta. Converte o x do mouse com o **mesmo** `editing_text_x` que a
    /// pintura usa.
    fn byte_at(
        &self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        point: (f32, f32),
    ) -> Option<usize> {
        let editing = self.editing.as_ref()?;
        let rect = self.field_rect(env, editing.block, editing.part)?;
        let m = self.metrics(env);
        let text = editing.state.text();
        let width = measurer.measure_width(text, BODY_FONT, m.field_font_size);
        let text_x = paint::editing_text_x(&m, rect, width);
        Some(measurer.index_at_offset(text, BODY_FONT, m.field_font_size, point.0 - text_x))
    }

    /// O arraste com o botão apertado dentro de um campo: estende a seleção.
    fn drag_selection(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, point: (f32, f32)) {
        if let Some(byte) = self.byte_at(env, measurer, point)
            && let Some(editing) = &mut self.editing
        {
            editing.state.drag_to(byte);
            self.window.request_redraw();
        }
    }

    /// Confirma o campo em edição: o texto vira pendência (ou recusa) no
    /// rascunho. Texto que não mudou desde o começo não cria nada.
    fn commit_edit(&mut self) {
        self.dragging_field = false;
        let Some(editing) = self.editing.take() else {
            return;
        };
        // O filtro já valeu a cada tecla: nada a confirmar no rascunho.
        if editing.part == EditPart::Filter {
            self.window.request_redraw();
            return;
        }
        if editing.changed()
            && let Some(option) = self.option_at(editing.block)
        {
            // Um valor recusado fica no rascunho, marcado na linha.
            match editing.part {
                EditPart::ListFirst(item) => {
                    interact::list_set_text(
                        &mut self.draft,
                        option,
                        item,
                        false,
                        editing.state.text(),
                    );
                }
                EditPart::ListSecond(item) => {
                    interact::list_set_text(
                        &mut self.draft,
                        option,
                        item,
                        true,
                        editing.state.text(),
                    );
                }
                EditPart::Field | EditPart::GitSeconds | EditPart::Filter => {
                    interact::commit_text(&mut self.draft, option, editing.state.text());
                }
            }
            self.invalidate();
        } else {
            self.window.request_redraw();
        }
    }

    /// `Up`/`Down` num campo numérico: o passo da opção sobre o texto, que é
    /// confirmado na hora -- o campo segue em edição.
    fn step_edit(&mut self, direction: i32) {
        let Some(editing) = &self.editing else {
            return;
        };
        let Some(option) = self.option_at(editing.block) else {
            return;
        };
        let (block, part) = (editing.block, editing.part);
        let Some(next) =
            interact::step_text(&mut self.draft, option, editing.state.text(), direction)
        else {
            return;
        };
        let mut stepped = Editing::new(block, part, next.clone());
        stepped.initial = next;
        self.editing = Some(stepped);
        self.invalidate();
    }

    /// `Up`/`Down` numa linha numérica com o foco, fora de edição.
    fn step_row(&mut self, index: usize, direction: i32) {
        if !self.file.allows_edit() {
            return;
        }
        let Some(option) = self.option_at(index) else {
            return;
        };
        let Some(ControlView::Field { text, .. }) = self.content().row(index).map(|r| &r.control)
        else {
            return;
        };
        let text = text.clone();
        if interact::step_text(&mut self.draft, option, &text, direction).is_some() {
            self.invalidate();
        }
    }

    /// Alterna uma opção booleana.
    fn toggle(&mut self, option: &OptionDef) {
        if interact::toggle(&mut self.draft, option) {
            self.invalidate();
        }
    }

    /// Escolhe o valor `segment` de um segmentado.
    fn choose_segment(&mut self, option: &OptionDef, segment: usize) {
        if interact::choose_segment(&mut self.draft, option, segment) {
            self.invalidate();
        }
    }

    /// A alternância do Git: desligar grava `0`; ligar volta ao padrão.
    fn toggle_git(&mut self, option: &OptionDef) {
        if interact::toggle_git(&mut self.draft, option) {
            self.invalidate();
        }
    }

    // ---- lista de escolha

    /// Abre a lista da linha `index`: os idiomas achados nos diretórios de
    /// `locales/` (com o do valor em vista, mesmo que o arquivo dele tenha
    /// sumido), ou os valores nomeados da opção.
    fn open_choice(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, index: usize) {
        let Some(option) = self.option_at(index) else {
            return;
        };
        let EditValue::String(current) = self.draft.value(option) else {
            return;
        };
        let values: Vec<String> = match option.control {
            Control::Language => {
                let mut found = language::available_languages(&self.locale_dirs);
                if !found.contains(&current) {
                    found.push(current.clone());
                    found.sort();
                }
                found
            }
            Control::Choice(choices) => choices.iter().map(|c| (*c).to_owned()).collect(),
            _ => return,
        };
        let Some(anchor) = self.control_rect(env, index) else {
            return;
        };
        let items = values
            .into_iter()
            .map(|value| {
                let label = match option.control {
                    Control::Language => value.clone(),
                    _ => choice_label(&self.catalog, &value),
                };
                ChoiceItem {
                    label: choice_list::fit_label(&label, env.config, anchor.width, measurer),
                    value,
                }
            })
            .collect();
        self.choice = Some(ChoiceList::open(index, items, &current));
        self.window.request_redraw();
    }

    /// Onde a lista aberta fica, ancorada ao controle da linha dela.
    fn choice_layout(&self, env: Env<'_>) -> Option<ChoiceLayout> {
        let list = self.choice.as_ref()?;
        let anchor = self.control_rect(env, list.block)?;
        Some(choice_list::layout(
            list,
            anchor,
            env.config,
            self.logical_width,
            self.logical_height,
        ))
    }

    /// Escolhe o item realçado da lista e a fecha.
    fn select_choice(&mut self) {
        let Some(list) = self.choice.take() else {
            return;
        };
        if let (Some(option), Some(value)) = (self.option_at(list.block), list.highlighted_value())
        {
            interact::choose_value(&mut self.draft, option, value);
            self.invalidate();
        }
        self.window.request_redraw();
    }

    /// Clique na drag region: duplo clique maximiza/restaura, e o resto
    /// arrasta a janela. Resolvido no *press*, antes de `drag_window` -- ela
    /// entrega o gesto ao loop modal do SO, sem garantia de ver o `Released`
    /// de volta (mesma razão de `WindowState::resolve_titlebar_drag`).
    fn resolve_titlebar_drag(&mut self) {
        let now = Instant::now();
        let is_double_click = self
            .last_titlebar_click
            .is_some_and(|at| now.duration_since(at) <= DOUBLE_CLICK_THRESHOLD);
        self.last_titlebar_click = if is_double_click { None } else { Some(now) };
        if is_double_click {
            self.window.set_maximized(!self.window.is_maximized());
        } else {
            let _ = self.window.drag_window();
        }
    }

    // ---- teclado

    pub(crate) fn modifiers_changed(&mut self, state: ModifiersState) {
        self.modifiers = modifiers_from(state);
    }

    /// `Ctrl` no Windows e no Linux, `Cmd` no macOS: o modificador de
    /// `Ctrl+S` (RF-16.10), o mesmo que o campo de texto lê para `Ctrl+A`.
    fn primary(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.modifiers.super_
        } else {
            self.modifiers.ctrl
        }
    }

    /// Teclas da janela (RF-16.10, ADR-0059 §3): modo de captura -- o mapa de
    /// teclas do processo não é consultado aqui. `Tab`/`Shift+Tab` percorrem
    /// guia, linhas e rodapé; as setas andam na guia e entre as opções de uma
    /// escolha; `Espaço` alterna; `Enter` confirma campo; `Ctrl+S` salva;
    /// `Esc` fecha -- ou, com um campo ou uma lista abertos, cancela só eles.
    pub(crate) fn key(
        &mut self,
        event: &KeyEvent,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
    ) -> Press {
        if event.state != winit::event::ElementState::Pressed {
            return Press::Nothing;
        }
        self.ensure_content(env, measurer);
        self.hover.dismiss();

        // O diálogo de pendências é modal: só ele lê o teclado.
        if self.dialog.is_some() {
            return self.dialog_key(&event.logical_key);
        }
        // A captura de atalho consome a próxima combinação inteira, inclusive
        // o que seria tecla da própria tela -- `Ctrl+S`, `Esc` (ADR-0059 §3).
        if self.capturing.is_some() {
            return self.capture_key(event);
        }

        let is_save = matches!(
            &event.logical_key,
            Key::Character(text) if text.eq_ignore_ascii_case("s")
        );
        if self.primary() && is_save {
            self.commit_edit();
            return if self.can_save() {
                Press::Save
            } else {
                Press::Nothing
            };
        }
        if self.choice.is_some() {
            self.key_choice(&event.logical_key);
            return Press::Nothing;
        }
        if self.editing.is_some() {
            self.key_editing(event, env);
            return Press::Nothing;
        }

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => self.request_close(),
            Key::Named(NamedKey::Tab) => {
                self.move_focus(env, self.modifiers.shift);
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowDown) if self.focus == Focus::Sidebar => {
                self.step_group(1);
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowUp) if self.focus == Focus::Sidebar => {
                self.step_group(-1);
                Press::Nothing
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => self.activate(env, measurer),
            Key::Named(NamedKey::ArrowLeft) => {
                self.step_segmented(-1);
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.step_segmented(1);
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowUp) => {
                if let Focus::Row(index) = self.focus {
                    self.step_row(index, 1);
                }
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowDown) => {
                if let Focus::Row(index) = self.focus {
                    self.step_row(index, -1);
                }
                Press::Nothing
            }
            _ => Press::Nothing,
        }
    }

    /// `Enter`/`Espaço` no que tem o foco: o botão do rodapé, ou o controle da
    /// linha -- alterna, começa a editar o campo, abre a lista.
    fn activate(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) -> Press {
        match self.focus {
            Focus::Footer(button) => self.press_footer(button),
            Focus::Banner(button) => self.press_banner(button),
            Focus::Row(index) => {
                self.activate_row(env, measurer, index);
                Press::Nothing
            }
            Focus::Sidebar => Press::Nothing,
        }
    }

    fn activate_row(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, index: usize) {
        // O filtro do grupo Atalhos: `Enter` começa a digitar.
        if matches!(self.content().blocks.get(index), Some(Block::Filter { .. })) {
            self.start_edit(index, EditPart::Filter);
            self.window.request_redraw();
            return;
        }
        // Somente leitura (RF-16.22): nada abre nem alterna.
        if !self.file.allows_edit() {
            return;
        }
        // Uma linha de atalho: `Enter` entra em captura do primeiro atalho --
        // ou acrescenta um, se a ação não tem nenhum (RF-16.29).
        if let Some(action) = self.content().row(index).and_then(|row| row.action) {
            let first = self.shortcuts.chords(action).first().copied();
            self.start_capture(action, first);
            return;
        }
        let Some(option) = self.option_at(index) else {
            return;
        };
        let Some(control) = self.content().row(index).map(|row| row.control.clone()) else {
            return;
        };
        match control {
            ControlView::Toggle { .. } => self.toggle(option),
            ControlView::GitPoll { on, .. } => {
                if on {
                    self.start_edit(index, EditPart::GitSeconds);
                } else {
                    self.toggle_git(option);
                }
            }
            ControlView::Field { .. } => self.start_edit(index, EditPart::Field),
            ControlView::Choice { .. } => self.open_choice(env, measurer, index),
            // Uma lista com itens abre o primeiro para edição; vazia, ganha o
            // primeiro item -- é o caminho do teclado até o "Adicionar".
            ControlView::List { items, .. } => {
                if items.is_empty() {
                    self.add_list_item(index, option);
                } else {
                    self.start_edit(index, EditPart::ListFirst(0));
                }
            }
            ControlView::Themes { .. } => {
                self.choose_theme(index);
            }
            ControlView::Segmented { .. } | ControlView::Chips { .. } => {}
        }
        self.window.request_redraw();
    }

    /// Começa a editar o campo da linha pelo teclado: o cursor no fim.
    fn start_edit(&mut self, index: usize, part: EditPart) {
        if let Some(initial) = self.edit_initial(index, part) {
            self.editing = Some(Editing::new(index, part, initial));
        }
    }

    /// Setas esquerda e direita sobre um segmentado com o foco.
    fn step_segmented(&mut self, delta: i32) {
        if !self.file.allows_edit() {
            return;
        }
        let Focus::Row(index) = self.focus else {
            return;
        };
        let Some(ControlView::Segmented {
            selected, labels, ..
        }) = self.content().row(index).map(|row| row.control.clone())
        else {
            return;
        };
        let next = (selected as i32 + delta).clamp(0, labels.len() as i32 - 1) as usize;
        if next != selected
            && let Some(option) = self.option_at(index)
        {
            self.choose_segment(option, next);
        }
    }

    /// Teclas com a lista aberta: setas movem o realce, `Enter` e `Espaço`
    /// escolhem, `Esc` fecha. O resto é engolido.
    fn key_choice(&mut self, key: &Key) {
        match key {
            Key::Named(NamedKey::Escape) => {
                self.choice = None;
                self.window.request_redraw();
            }
            Key::Named(NamedKey::ArrowUp) => {
                if let Some(list) = &mut self.choice {
                    list.move_highlight(-1);
                }
                self.window.request_redraw();
            }
            Key::Named(NamedKey::ArrowDown) => {
                if let Some(list) = &mut self.choice {
                    list.move_highlight(1);
                }
                self.window.request_redraw();
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => self.select_choice(),
            _ => {}
        }
    }

    /// Teclas com um campo em edição: `Enter` confirma, `Esc` descarta a
    /// digitação, `Tab` confirma e segue, `Up`/`Down` somam o passo num campo
    /// numérico, e o resto edita o texto.
    fn key_editing(&mut self, event: &KeyEvent, env: Env<'_>) {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.editing = None;
                self.dragging_field = false;
                self.window.request_redraw();
            }
            Key::Named(NamedKey::Enter) => self.commit_edit(),
            Key::Named(NamedKey::Tab) => {
                // Numa lista, `Tab` anda pelos campos dos itens; nas pontas
                // dela sai como de qualquer campo.
                if !self.walk_list_field(self.modifiers.shift) {
                    self.commit_edit();
                    self.move_focus(env, self.modifiers.shift);
                }
            }
            // `Alt+Up`/`Alt+Down` reordenam o item em edição (RF-16.26).
            Key::Named(NamedKey::ArrowUp) if self.modifiers.alt => self.move_list_item(-1),
            Key::Named(NamedKey::ArrowDown) if self.modifiers.alt => self.move_list_item(1),
            Key::Named(NamedKey::ArrowUp) => self.step_edit(1),
            Key::Named(NamedKey::ArrowDown) => self.step_edit(-1),
            Key::Named(NamedKey::Backspace) => {
                if let Some(editing) = &mut self.editing {
                    editing.state.backspace();
                }
                self.window.request_redraw();
            }
            key => {
                if let Some(editing) = &mut self.editing
                    && apply_text_field_key(
                        &mut editing.state,
                        key,
                        event.text.as_deref(),
                        self.modifiers,
                    )
                {
                    self.window.request_redraw();
                }
            }
        }
        self.sync_filter();
    }

    /// O filtro vale a cada tecla: o texto do campo em edição vira o filtro e
    /// a lista é refeita (RF-16.28).
    fn sync_filter(&mut self) {
        let Some(editing) = &self.editing else {
            return;
        };
        if editing.part == EditPart::Filter && editing.state.text() != self.filter {
            self.filter = editing.state.text().to_owned();
            self.scroll = 0.0;
            self.invalidate();
        }
    }

    // ---- atalhos (RF-16.28 a RF-16.31)

    /// Entra em captura na linha de `action`: o chip `replacing` é o que a
    /// próxima combinação substitui, ou -- sem ele -- um atalho a acrescentar.
    fn start_capture(&mut self, action: Action, replacing: Option<Chord>) {
        self.commit_edit();
        self.choice = None;
        self.capturing = Some(Capturing {
            action,
            replacing,
            frozen: self.shortcuts.chords(action),
            conflict: None,
        });
        self.invalidate();
    }

    /// Clique no chip `chip` da linha `block`: o chip que mostra uma combinação
    /// a substitui; o "Nenhum atalho" e o chip em captura acrescentam um.
    fn click_chip(&mut self, block: usize, chip: usize, action: Action) {
        let shown = match &self.capturing {
            Some(capturing) if capturing.action == action => capturing.frozen.clone(),
            _ => self.shortcuts.chords(action),
        };
        let tone = match self.content().row(block).map(|row| &row.control) {
            Some(ControlView::Chips { chips }) => chips.get(chip).map(|chip| chip.tone),
            _ => None,
        };
        let replacing = match tone {
            Some(ChipTone::Normal) => shown.get(chip).copied(),
            _ => None,
        };
        self.start_capture(action, replacing);
    }

    /// Substituir (`true`) ou Cancelar (`false`) o conflito pendente
    /// (RF-16.30): substituir dá a combinação à ação e a tira da outra.
    fn answer_conflict(&mut self, replace: bool) {
        let Some(capturing) = self.capturing.take() else {
            return;
        };
        if replace && let Some(conflict) = capturing.conflict {
            self.shortcuts
                .bind(capturing.action, capturing.replacing, conflict.chord);
        }
        self.invalidate();
    }

    /// Uma tecla com a captura em curso (RF-16.29): `Esc` cancela, `Backspace`
    /// remove o atalho, modificador sozinho, tecla morta e o que a gramática
    /// não escreve são ignorados, e uma combinação vira o atalho -- ou, se é
    /// de outra ação, um conflito à espera de resposta.
    fn capture_key(&mut self, event: &KeyEvent) -> Press {
        if event.repeat {
            return Press::Nothing;
        }
        let Some(capturing) = self.capturing.clone() else {
            return Press::Nothing;
        };
        if capturing.conflict.is_some() {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => self.answer_conflict(false),
                Key::Named(NamedKey::Enter) => self.answer_conflict(true),
                _ => {}
            }
            return Press::Nothing;
        }
        match Chord::capture(&event.logical_key, self.modifiers) {
            Capture::Ignored => return Press::Nothing,
            Capture::Cancel => self.capturing = None,
            Capture::Remove => {
                if let Some(old) = capturing.replacing {
                    self.shortcuts.unbind(capturing.action, old);
                }
                self.capturing = None;
            }
            Capture::Chord(chord) => match self.shortcuts.holder(chord) {
                // Já é dela: nada muda.
                Some(holder) if holder == capturing.action => self.capturing = None,
                Some(other) => {
                    if let Some(state) = &mut self.capturing {
                        state.conflict = Some(Conflict { chord, other });
                    }
                }
                None => {
                    self.shortcuts
                        .bind(capturing.action, capturing.replacing, chord);
                    self.capturing = None;
                }
            },
        }
        self.invalidate();
        Press::Nothing
    }

    /// `Tab`/`Shift+Tab` num campo de lista: confirma o campo e passa para o
    /// próximo -- nome, valor, próximo item. Devolve `false` nas pontas da
    /// lista, ou fora de uma, e quem chama segue como de qualquer campo.
    fn walk_list_field(&mut self, backwards: bool) -> bool {
        let Some(editing) = &self.editing else {
            return false;
        };
        let block = editing.block;
        let Some(ControlView::List {
            items, two_fields, ..
        }) = self.content().row(block).map(|row| &row.control)
        else {
            return false;
        };
        let Some(next) = editing
            .part
            .next_in_list(items.len(), *two_fields, backwards)
        else {
            return false;
        };
        self.commit_edit();
        if let Some(initial) = self.edit_initial(block, next) {
            self.editing = Some(Editing::new(block, next, initial));
        }
        self.window.request_redraw();
        true
    }

    /// `Alt+Up`/`Alt+Down` com um item de lista em edição: o confirma e o move
    /// uma posição, e a edição o segue. Na ponta da lista não faz nada.
    fn move_list_item(&mut self, delta: i32) {
        let Some(editing) = &self.editing else {
            return;
        };
        let (block, part) = (editing.block, editing.part);
        let Some(item) = part.list_item() else {
            return;
        };
        let Some(option) = self.option_at(block) else {
            return;
        };
        let text = editing.state.text().to_owned();
        let second = matches!(part, EditPart::ListSecond(_));
        interact::list_set_text(&mut self.draft, option, item, second, &text);
        let moved = interact::list_move(&mut self.draft, option, item, delta);
        let part = part.at_item(moved.unwrap_or(item));
        // O texto já está no rascunho: a edição recomeça dele, no item novo.
        self.editing = Some(Editing::new(block, part, text));
        self.invalidate();
    }

    /// Passa o foco ao próximo ponto de parada e leva a linha à vista.
    fn move_focus(&mut self, env: Env<'_>, backwards: bool) {
        let content = self.content();
        let order = focus_order_with_banner(
            &self.banner_kinds(),
            &content.row_indices(),
            &self.available_buttons(),
        );
        self.focus = next_focus(&order, self.focus, backwards);
        if let Focus::Row(index) = self.focus {
            let layout = self.layout(env);
            let content = self.content();
            if let Some(BlockGeometry::Row(row)) = content.geometry.blocks.get(index) {
                self.scroll = layout::clamp_scroll(
                    layout::scroll_to_reveal(
                        self.scroll,
                        row.rect.y,
                        row.rect.height,
                        layout.panel_body.height,
                    ),
                    content.geometry.content_height,
                    layout.panel_body.height,
                );
            }
        }
        self.window.request_redraw();
    }

    /// Seta na guia: o grupo vizinho, parando nas pontas.
    fn step_group(&mut self, delta: i32) {
        let index = Group::ALL
            .iter()
            .position(|group| *group == self.selected_group)
            .unwrap_or(0) as i32;
        let next = (index + delta).clamp(0, Group::ALL.len() as i32 - 1) as usize;
        self.select_group(Group::ALL[next]);
    }

    /// Escolhe o grupo: o painel volta ao topo (RF-16.9). O campo em edição
    /// é confirmado antes -- ele é do grupo que se deixa.
    fn select_group(&mut self, group: Group) {
        if group != self.selected_group {
            self.commit_edit();
            self.choice = None;
            self.capturing = None;
            self.selected_group = group;
            self.scroll = 0.0;
            self.hover.dismiss();
        }
        if matches!(self.focus, Focus::Row(_)) {
            self.focus = Focus::Sidebar;
        }
        self.window.request_redraw();
    }

    // ---- tooltip no relógio do processo

    pub(crate) fn next_wake(&self) -> Option<Instant> {
        self.hover.next_deadline()
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        let was_visible = self.hover.visible().is_some();
        self.hover.tick(now);
        if was_visible != self.hover.visible().is_some() {
            self.window.request_redraw();
        }
    }

    // ---- acessibilidade

    /// Monta a árvore de acessibilidade e a entrega -- só se houver cliente
    /// conectado, e nunca pede quadro (ADR-0043 §3). Parte do mesmo layout e
    /// do mesmo conteúdo que a pintura lê.
    pub(crate) fn refresh_access_tree(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        language: &str,
    ) {
        self.ensure_content(env, measurer);
        let m = self.metrics(env);
        let layout = self.layout(env);
        let groups: Vec<Group> =
            group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height)
                .into_iter()
                .map(|(group, _)| group)
                .collect();
        let selected = self.selected_group;
        let has_pending = self.has_pending_changes();
        let catalog = &self.catalog;
        let dialog = self.dialog.as_ref();
        let filter = (selected == Group::Shortcuts).then_some(self.filter.as_str());
        let banner = self
            .content
            .as_ref()
            .and_then(|content| content.banner.as_ref());
        let rows: Vec<&RowView> = self
            .content
            .as_ref()
            .map(|content| {
                content
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Row(row) => Some(&**row),
                        Block::Section(_) | Block::Note { .. } | Block::Filter { .. } => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.access_adapter.update_if_active(|| {
            access::build_settings_tree(
                &layout,
                &groups,
                selected,
                has_pending,
                &rows,
                filter,
                banner,
                dialog,
                catalog,
                language,
            )
        });
    }

    // ---- pintura

    /// Pinta o quadro: painel ao fundo, guia, corpo e rodapé, e -- fora do
    /// macOS -- o cabeçalho por cima. Tudo na camada `Chrome` (ADR-0060 §4); o
    /// tooltip e a lista de escolha na `Popover`. Camada nova nenhuma.
    pub(crate) fn paint(
        &mut self,
        env: Env<'_>,
        pal: &ResolvedPalette,
        term_pal: &ResolvedTermPalette,
        measurer: &mut TextMeasurer,
    ) -> Frame {
        self.ensure_content(env, measurer);
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);
        let mut pending_groups = self.draft.pending_groups();
        if self.shortcuts.is_dirty() {
            pending_groups.push(Group::Shortcuts);
        }

        let mut out = vec![
            quad(layout.panel, PANEL_BACKGROUND),
            quad(layout.sidebar, pal.bar_background),
            quad(layout.sidebar_separator, pal.editor_divider),
        ];
        out.extend(paint::paint_body(
            &paint::Input {
                layout: &layout,
                metrics: &m,
                content,
                items: &items,
                footer: &footer,
                selected: self.selected_group,
                focus: self.focus,
                hovered: self.hovered,
                scroll: self.scroll,
                pending_groups: &pending_groups,
                editing: self.editing.as_ref(),
                selection_color: term_pal.selection_background,
                footer_available: self.available_footer(),
                read_only: !self.file.allows_edit(),
                style: env.style,
                pal,
                config: env.config,
                catalog: &self.catalog,
            },
            measurer,
        ));
        if let Some(header) = layout.header {
            out.push(quad(header, pal.bar_background));
            self.paint_header_content(header, env.style, pal, &mut out);
            chrome::paint_window_buttons(
                env.style,
                pal,
                self.logical_width,
                header.height,
                self.window.is_maximized(),
                self.hovered_button,
                &mut out,
            );
        }
        let mut frame = Frame::new();
        frame.set_layer(Layer::Chrome, out);
        let mut popover = Vec::new();
        if let Some((anchor, text)) = self.hover.visible() {
            popover.extend(overlay::paint_tooltip(
                anchor,
                text,
                env.config,
                pal,
                self.logical_width,
                self.logical_height,
                measurer,
            ));
        }
        if let (Some(list), Some(list_layout)) = (&self.choice, self.choice_layout(env)) {
            popover.extend(choice_list::paint(list, &list_layout, env.config, pal));
        }
        if !popover.is_empty() {
            frame.set_layer(Layer::Popover, popover);
        }
        // O diálogo de pendências, na camada modal desta janela (ADR-0060 §4).
        if let (Some(dialog), Some(layout)) = (&self.dialog, self.dialog_layout(env, measurer)) {
            frame.set_layer(
                Layer::Modal,
                overlay::paint_dialog(
                    &layout,
                    dialog,
                    env.config,
                    pal,
                    self.logical_width,
                    self.logical_height,
                    measurer,
                ),
            );
        }
        frame
    }

    /// Ícone e título à esquerda do cabeçalho (ADR-0060 §1): no
    /// `trilha_padding` mais o `padding_left` da aba, o ícone `SETTINGS` na
    /// em de ícone do chrome × `0.8` -- o multiplicador do botão de
    /// configurações da barra --, e o título 15px/500 depois de um `gap: 8`.
    fn paint_header_content(
        &self,
        header: Rect,
        style: &TabBarStyle,
        pal: &ResolvedPalette,
        out: &mut Vec<Primitive>,
    ) {
        let icon_size = style.icon_em_size * 0.8;
        let icon_x = style.trilha_padding + style.padding_left;
        let icon_width = icon::SETTINGS.ink_width(icon_size);
        out.push(chrome::centered_glyph(
            icon::SETTINGS,
            Rect {
                x: icon_x,
                y: header.y,
                width: icon_width,
                height: header.height,
            },
            icon_size,
            pal.chrome_icon,
        ));
        out.push(Primitive::Text(TextRun {
            origin: (
                icon_x + icon_width + HEADER_GAP_PX,
                header.y + (header.height - TITLE_SIZE_PX) / 2.0,
            ),
            text: msg::settings::window_title(&self.catalog),
            font: overlay::TITLE_FONT,
            size_px: TITLE_SIZE_PX,
            color: pal.dialog_title_text,
        }));
    }

    /// Submete o quadro. O fundo de limpeza é o do painel, que é o que
    /// ocupa a maior parte da janela.
    pub(crate) fn render(&mut self, gpu: &mut GpuContext, frame: &Frame) {
        self.surface.render(gpu, PANEL_BACKGROUND, frame);
    }
}

fn quad(rect: Rect, color: porecatu_render::Color) -> Primitive {
    Primitive::Quad(Quad { rect, color })
}
