// SPDX-License-Identifier: GPL-3.0-or-later

//! `ProfileEditorWindow`: a janela do SO do editor de perfil (ADR-0080 §1 a
//! §6). É filha e modal da tela de configurações e **campo dela**
//! (`SettingsWindow::profile_editor`), não de `App`: o editor não tem sentido
//! sem a tela e cai com ela.
//!
//! Guarda a própria `WindowSurface` -- o `GpuContext` e o atlas de glyphs são
//! os do processo, como para toda janela --, o `ProfileEditor` (a cópia do
//! perfil, estado puro de `profile_editor`), o formulário em coluna medido
//! (`profile_form`) e o estado de ponteiro, de foco e de teclado. Como a tela,
//! **só produz pedidos**: um clique ou uma tecla devolve [`EditorPress`], e
//! quem possui a janela (a tela) decide -- OK leva a cópia ao rascunho,
//! Cancelar a larga. Nada é escrito no disco daqui, e **toda edição vai à
//! cópia**, nunca ao `ProfilesDraft`, que só se lê (os `id` que a cópia não
//! pode repetir, e a validação de OK).
//!
//! **Foco e edição.** O foco do teclado é uma linha do formulário ou um botão
//! do rodapé; `Tab`/`Shift+Tab` percorrem as linhas e depois os botões. O foco
//! num campo de texto já é a edição dele (`Enter` ali é OK, que é o que um
//! formulário faz); nas outras linhas é só o anel, e `Enter` as aciona -- abre a
//! escolha, esquece o segredo, entra na lista.
//!
//! **Medir texto é caro.** O formulário (texto cortado, larguras) é medido uma
//! vez e guardado; só volta a ser medido quando a largura da janela, o idioma
//! ou a cópia mudam. Nenhum caminho de pintura ou de hit-test mede texto, salvo
//! o do campo que está recebendo teclas.

use std::sync::Arc;
use std::time::Instant;

use porecatu_config::ProfileKind;
use porecatu_locale::Catalog;
use porecatu_render::{
    Frame, GpuContext, Layer, Primitive, Quad, Rect, TextMeasurer, WindowSurface,
};
use porecatu_term::Modifiers;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use super::choice_list::{self, ChoiceLayout, ChoiceList};
use super::content::{self, ControlView, ProfileClick, ProfileTarget};
use super::field_edit::{EditPart, Editing};
use super::layout::{
    self, ControlPart, Hit, Layout, Metrics, left_aligned_buttons, right_aligned_buttons,
};
use super::paint::{self, ControlCtx, FooterButtonView, paint_button};
use super::profile_editor::{EditorProblems, Focus as ProblemFocus, ProfileEditor};
use super::profile_form::{FormView, is_text_field};
use super::profiles_draft::{Field, Forget, ListKind, ProfilesDraft, SecretKind};
use super::window::{Env, forget_in_vault, header_height, paint_header_title, titlebar_drag};
use crate::input::modifiers_from;
use crate::messages::msg;
use crate::overlay::BODY_FONT;
use crate::palette::{ResolvedPalette, ResolvedTermPalette};
use crate::tab_bar::{self, WindowButtonHit};
use crate::text_field::apply_text_field_key;
use crate::{access, bar_height, chrome, is_macos, titlebar};

/// O único botão de janela do editor (ADR-0080 §3).
const SHOWN_WINDOW_BUTTONS: [WindowButtonHit; 1] = [WindowButtonHit::Close];

/// Os botões do rodapé, da esquerda para a direita: é a ordem de leitura e de
/// `Tab` (ADR-0080 §5). Os dois "Esquecer" ficam à esquerda e só existem num
/// perfil SSH com cofre do sistema; Cancelar e OK, à direita.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorButton {
    ForgetPassword,
    ForgetPassphrase,
    Cancel,
    Ok,
}

pub(crate) const EDITOR_BUTTONS: [EditorButton; 4] = [
    EditorButton::ForgetPassword,
    EditorButton::ForgetPassphrase,
    EditorButton::Cancel,
    EditorButton::Ok,
];

impl EditorButton {
    /// O texto do botão, no idioma do catálogo.
    pub(crate) fn label(self, catalog: &Catalog) -> String {
        use msg::settings::profiles as p;
        match self {
            Self::ForgetPassword => p::forget_password(catalog),
            Self::ForgetPassphrase => p::forget_passphrase(catalog),
            Self::Cancel => msg::settings::dialog::cancel(catalog),
            Self::Ok => p::editor_ok(catalog),
        }
    }

    /// O segredo que o botão esquece, se é um "Esquecer".
    fn secret(self) -> Option<SecretKind> {
        match self {
            Self::ForgetPassword => Some(SecretKind::Password),
            Self::ForgetPassphrase => Some(SecretKind::Passphrase),
            Self::Cancel | Self::Ok => None,
        }
    }
}

/// O que o cofre tem guardado para o perfil que a cópia é agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Secrets {
    pub(super) password: bool,
    pub(super) passphrase: bool,
}

/// Os botões que o rodapé tem, da esquerda para a direita, e se cada um está
/// disponível. Os "Esquecer" só existem num perfil SSH com cofre, e ficam
/// **desabilitados** sem o que esquecer -- esmaecidos, nunca ausentes, e fora do
/// `Tab`; Cancelar e OK estão sempre.
pub(super) fn footer_buttons(
    ssh: bool,
    vault: bool,
    secrets: Secrets,
) -> Vec<(EditorButton, bool)> {
    let mut buttons = Vec::new();
    if ssh && vault {
        buttons.push((EditorButton::ForgetPassword, secrets.password));
        buttons.push((EditorButton::ForgetPassphrase, secrets.passphrase));
    }
    buttons.push((EditorButton::Cancel, true));
    buttons.push((EditorButton::Ok, true));
    buttons
}

/// Há esse segredo no cofre? Só o sim ou não: a UI nunca vê o segredo.
fn secret_exists(target: &Forget) -> bool {
    match target {
        Forget::Password(destination) => porecatu_term::has_password(destination),
        Forget::Passphrase(path) => porecatu_term::has_passphrase(path),
    }
}

/// O que um clique ou uma tecla pede a quem possui a janela. Fechar pelo botão,
/// pelo gesto do sistema e por `Esc` é Cancelar (ADR-0080 §3, §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorPress {
    Nothing,
    Cancel,
    Ok,
}

/// Quem tem o foco do teclado: uma linha do formulário (pelo índice) ou um
/// botão do rodapé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorFocus {
    Row(usize),
    Button(EditorButton),
}

/// O alvo que um press esquerdo armou (ADR-0081): só age se o release cai no
/// mesmo alvo.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Armed {
    /// Um botão do rodapé.
    Button(EditorButton),
    /// Um botão de uma linha (autenticação, `X`, "Adicionar").
    Control(usize, ControlPart),
    /// O item `n` da lista aberta.
    ChoiceItem(usize),
}

/// A ordem do `Tab`: as `rows` linhas do formulário, depois os botões do rodapé
/// (`buttons`, de [`footer_buttons`]) da esquerda para a direita (ADR-0080 §5).
/// Um botão desabilitado não é parada.
pub(super) fn focus_order(rows: usize, buttons: &[(EditorButton, bool)]) -> Vec<EditorFocus> {
    (0..rows)
        .map(EditorFocus::Row)
        .chain(
            buttons
                .iter()
                .filter(|(_, available)| *available)
                .map(|(button, _)| EditorFocus::Button(*button)),
        )
        .collect()
}

/// O foco que `Tab` (ou `Shift+Tab`) leva a partir de `current`, dando a volta
/// nas pontas. Sem foco -- ou com um que saiu da ordem --, o primeiro (ou o
/// último, com `Shift`).
pub(super) fn next_in_order(
    order: &[EditorFocus],
    current: Option<EditorFocus>,
    backwards: bool,
) -> EditorFocus {
    let last = order.len() - 1;
    let at = current.and_then(|focus| order.iter().position(|candidate| *candidate == focus));
    let next = match (at, backwards) {
        (None, false) => 0,
        (None, true) => last,
        (Some(at), false) => (at + 1) % order.len(),
        (Some(at), true) => (at + order.len() - 1) % order.len(),
    };
    order[next]
}

/// O que `Esc` faz num campo em edição (ADR-0080 §5: "quando nenhum campo ou
/// lista está consumindo o `Esc`"). Como o foco num campo já é a edição dele,
/// um `Esc` que sempre fosse do campo nunca cancelaria: o campo o consome
/// **só** enquanto tem texto digitado por confirmar -- desfaz a digitação --, e
/// sem ele o `Esc` é Cancelar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EscAction {
    RevertTyping,
    Cancel,
}

pub(super) fn esc_action(field_has_unconfirmed_text: bool) -> EscAction {
    if field_has_unconfirmed_text {
        EscAction::RevertTyping
    } else {
        EscAction::Cancel
    }
}

/// `Enter` num campo de uma linha é OK; numa lista segue o que a lista faz na
/// tela -- confirma o item e fica (ADR-0080 §5).
pub(super) fn enter_is_ok(target: Option<ProfileTarget>) -> bool {
    matches!(target, Some(ProfileTarget::Field(_)))
}

/// A linha (e o campo de lista, se for o caso) que um problema de OK aponta.
/// O erro de um item de ambiente leva ao nome do item.
pub(super) fn problem_row(
    view: &FormView,
    focus: ProblemFocus,
) -> Option<(usize, Option<EditPart>)> {
    match focus {
        ProblemFocus::Field(Field::Auth) => view.row_of(ProfileTarget::Auth).map(|row| (row, None)),
        ProblemFocus::Field(field) => view
            .row_of(ProfileTarget::Field(field))
            .map(|row| (row, None)),
        ProblemFocus::EnvItem(item) => view
            .row_of(ProfileTarget::List(ListKind::Env))
            .map(|row| (row, Some(EditPart::ListFirst(item)))),
    }
}

/// De onde sai o título (ADR-0080 §3). Guardado na abertura: o título de um
/// perfil existente é o nome que ele tinha, não o que se digita depois.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TitleSource {
    Edit(String),
    NewLocal,
    NewSsh,
}

impl TitleSource {
    fn of(editor: &ProfileEditor) -> Self {
        match (editor.is_new(), editor.kind()) {
            (false, _) => Self::Edit(editor.form().name.clone()),
            (true, ProfileKind::Local) => Self::NewLocal,
            (true, ProfileKind::Ssh) => Self::NewSsh,
        }
    }

    fn text(&self, catalog: &Catalog) -> String {
        use msg::settings::profiles as p;
        match self {
            Self::Edit(name) => p::editor_title(catalog, name),
            Self::NewLocal => p::editor_new_local(catalog),
            Self::NewSsh => p::editor_new_ssh(catalog),
        }
    }
}

pub(crate) struct ProfileEditorWindow {
    window: Arc<Window>,
    surface: WindowSurface,
    /// Pixels físicos por pixel lógico; tudo aqui trabalha em lógico.
    scale: f32,
    logical_width: f32,
    logical_height: f32,
    /// Quadros que ainda faltam redesenhar sem que nada tenha mudado (o
    /// primeiro present de uma surface recém-criada não chega à tela).
    settle_frames: u8,
    /// Última posição do cursor, em pixels físicos.
    cursor_position: (f64, f64),
    hovered_window_button: Option<WindowButtonHit>,
    /// O botão de janela armado pelo press (ADR-0066): dispara no release.
    pressed_window_button: Option<WindowButtonHit>,
    /// O botão, controle ou item da lista sob o press esquerdo (ADR-0081): o
    /// press só arma, e o release sobre o mesmo alvo é que age.
    armed: Option<Armed>,
    last_titlebar_click: Option<Instant>,
    catalog: Arc<Catalog>,
    /// Adaptador de acessibilidade, criado antes de a janela ficar visível.
    access_adapter: accesskit_winit::Adapter,
    editor: ProfileEditor,
    title: TitleSource,
    /// O que o cofre tem para o perfil que a cópia é agora (para os botões
    /// "Esquecer"), e se isso precisa ser perguntado de novo -- na abertura e
    /// quando o destino ou o arquivo de chave mudam, porque as contas do cofre
    /// são `usuário@host:porta` e o caminho da chave.
    secrets: Secrets,
    secrets_stale: bool,
    /// O formulário medido; `None` até o primeiro uso e depois de cada
    /// mudança na cópia, no idioma ou na largura.
    form: Option<FormView>,
    form_width_bits: u32,
    /// Rolagem vertical do corpo, em pixels lógicos de conteúdo.
    scroll: f32,
    focus: Option<EditorFocus>,
    /// O alvo sob o cursor, na linha ou no botão do rodapé.
    hovered: Option<Hit>,
    hovered_button: Option<EditorButton>,
    /// O campo que está recebendo teclas, se algum.
    editing: Option<Editing>,
    /// O botão do mouse segue apertado dentro de um campo: o arraste
    /// seleciona texto (ADR-0035).
    dragging_field: bool,
    /// A lista de escolha (a autenticação), aberta.
    choice: Option<ChoiceList>,
    modifiers: Modifiers,
    /// A largura de cada botão do rodapé, medida uma vez (e de novo quando o
    /// idioma muda).
    footer_widths: Option<[f32; 4]>,
    /// O primeiro uso do formulário leva o foco ao primeiro campo (§5).
    initial_focus: bool,
    /// Os erros só aparecem depois de um OK recusado (ADR-0080 §6); dali em
    /// diante acompanham a cópia, e o foco vai ao primeiro campo inválido.
    show_problems: bool,
    pending_focus: Option<ProblemFocus>,
}

impl ProfileEditorWindow {
    pub(crate) fn new(
        window: Arc<Window>,
        surface: WindowSurface,
        scale: f32,
        catalog: Arc<Catalog>,
        access_adapter: accesskit_winit::Adapter,
        editor: ProfileEditor,
    ) -> Self {
        let size = window.inner_size();
        Self {
            window,
            surface,
            scale,
            logical_width: size.width as f32 / scale,
            logical_height: size.height as f32 / scale,
            settle_frames: 1,
            cursor_position: (0.0, 0.0),
            hovered_window_button: None,
            pressed_window_button: None,
            armed: None,
            last_titlebar_click: None,
            catalog,
            access_adapter,
            title: TitleSource::of(&editor),
            editor,
            secrets: Secrets::default(),
            secrets_stale: true,
            form: None,
            form_width_bits: 0,
            scroll: 0.0,
            focus: None,
            hovered: None,
            hovered_button: None,
            editing: None,
            dragging_field: false,
            choice: None,
            modifiers: Modifiers::NONE,
            footer_widths: None,
            initial_focus: true,
            show_problems: false,
            pending_focus: None,
        }
    }

    /// O título que a janela de `editor` terá, no idioma de `catalog`: o
    /// `App` o põe nos atributos da janela, antes de ela existir.
    pub(crate) fn title_for(editor: &ProfileEditor, catalog: &Catalog) -> String {
        TitleSource::of(editor).text(catalog)
    }

    /// O título da janela, no idioma em uso.
    pub(crate) fn title_text(&self) -> String {
        self.title.text(&self.catalog)
    }

    pub(crate) fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub(crate) fn editor(&self) -> &ProfileEditor {
        &self.editor
    }

    /// OK foi recusado (ADR-0080 §6): o editor não fecha, as notas de erro
    /// aparecem abaixo dos campos e o foco vai ao primeiro campo inválido --
    /// rolando até ele. O foco é resolvido no próximo uso do formulário, que
    /// precisa estar medido.
    pub(crate) fn refuse(&mut self, problems: &EditorProblems) {
        self.show_problems = true;
        self.pending_focus = problems.first;
        self.form = None;
        self.window.request_redraw();
    }

    /// ADR-0043 §1: o adaptador precisa ver todo `WindowEvent` da janela,
    /// antes de ele ser tratado.
    pub(crate) fn process_access_event(&mut self, event: &WindowEvent) {
        self.access_adapter.process_event(&self.window, event);
    }

    /// Traz o editor para a frente: pedir para editar com um aberto o traz, e
    /// onde a modalidade é lógica um clique na tela também.
    pub(crate) fn bring_to_front(&self) {
        if self.window.is_minimized() == Some(true) {
            self.window.set_minimized(false);
        }
        self.window.focus_window();
    }

    pub(crate) fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub(crate) fn is_minimized(&self) -> bool {
        self.window.is_minimized() == Some(true)
    }

    /// A janela voltou à tela (a dona foi restaurada): dois quadros, a mesma
    /// causa do `settle_frames`.
    pub(crate) fn refresh_after_show(&mut self) {
        self.settle_frames = 1;
        self.window.request_redraw();
    }

    /// Troca de idioma ao vivo (ADR-0056 §9): o título do SO acompanha agora, e
    /// o formulário e os botões do rodapé são medidos de novo no próximo uso.
    pub(crate) fn set_catalog(&mut self, catalog: &Arc<Catalog>) {
        self.catalog = Arc::clone(catalog);
        self.footer_widths = None;
        self.form = None;
        self.choice = None;
        self.window.set_title(&self.title_text());
        self.window.request_redraw();
    }

    /// Acompanha o tamanho físico novo da janela (mesma regra da tela:
    /// minimizada o Windows relata o tamanho do ícone).
    pub(crate) fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32) {
        if !crate::window_size_is_usable(width, height, self.window.is_minimized()) {
            return;
        }
        self.surface.resize(gpu, width, height, self.scale);
        self.settle_frames = 1;
        self.logical_width = width as f32 / self.scale;
        self.logical_height = height as f32 / self.scale;
        // A lista é ancorada à linha; a largura nova refaz o formulário.
        self.choice = None;
        self.window.request_redraw();
    }

    pub(crate) fn rescale(&mut self, gpu: &GpuContext, scale: f32) {
        self.scale = scale;
        let size = self.window.inner_size();
        self.resize(gpu, size.width, size.height);
    }

    // ---- geometria

    fn metrics(env: Env<'_>) -> Metrics {
        Metrics::from_config(env.config, header_height(env.style))
    }

    /// As faixas: o cabeçalho, o corpo e o rodapé, pela mesma função da tela
    /// (sem guia e sem faixa de arquivo).
    fn layout(&self, env: Env<'_>) -> Layout {
        let m = Self::metrics(env);
        layout::layout_with_banner(
            self.logical_width,
            self.logical_height,
            m.header_height,
            0.0,
            m.footer_height(),
            0.0,
        )
    }

    fn cursor_logical(&self) -> (f32, f32) {
        (
            self.cursor_position.0 as f32 / self.scale,
            self.cursor_position.1 as f32 / self.scale,
        )
    }

    fn ensure_footer_widths(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) -> [f32; 4] {
        if let Some(widths) = self.footer_widths {
            return widths;
        }
        let m = Self::metrics(env);
        let widths = EDITOR_BUTTONS.map(|button| {
            measurer.measure_width(&button.label(&self.catalog), BODY_FONT, m.button_font_size)
                + m.button_padding_x * 2.0
        });
        self.footer_widths = Some(widths);
        widths
    }

    /// Pergunta ao cofre o que ele tem para o perfil, se ainda não perguntou
    /// para este destino. Só com cofre e num perfil SSH; `vault_available` pode
    /// responder `false` na primeira vez (ele sonda numa thread, para a tela
    /// nunca esperar o cofre), e então a pergunta fica para o próximo quadro.
    fn sync_secrets(&mut self) {
        if !self.secrets_stale || self.editor.kind() != ProfileKind::Ssh {
            return;
        }
        if !porecatu_term::vault_available() {
            return;
        }
        let exists = |kind| {
            self.editor
                .forget_target(kind)
                .is_some_and(|target| secret_exists(&target))
        };
        self.secrets = Secrets {
            password: exists(SecretKind::Password),
            passphrase: exists(SecretKind::Passphrase),
        };
        self.secrets_stale = false;
    }

    /// Os botões do rodapé agora e se cada um está disponível.
    fn footer_state(&mut self) -> Vec<(EditorButton, bool)> {
        self.sync_secrets();
        footer_buttons(
            self.editor.kind() == ProfileKind::Ssh,
            porecatu_term::vault_available(),
            self.secrets,
        )
    }

    /// Os retângulos dos botões do rodapé que existem, com a disponibilidade de
    /// cada um: os "Esquecer" colados à esquerda, Cancelar e OK à direita.
    fn footer_rects(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
    ) -> Vec<(EditorButton, Rect, bool)> {
        let widths = self.ensure_footer_widths(env, measurer);
        let m = Self::metrics(env);
        let footer = self.layout(env).footer;
        let [password, passphrase] = left_aligned_buttons(&m, footer, [widths[0], widths[1]]);
        let [cancel, ok] = right_aligned_buttons(&m, footer, [widths[2], widths[3]]);
        let rects = [password, passphrase, cancel, ok];
        self.footer_state()
            .into_iter()
            .map(|(button, available)| {
                let at = EDITOR_BUTTONS
                    .iter()
                    .position(|candidate| *candidate == button)
                    .unwrap_or(0);
                (button, rects[at], available)
            })
            .collect()
    }

    /// O botão do rodapé sob `point` e se está disponível.
    fn button_at(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        point: (f32, f32),
    ) -> Option<(EditorButton, bool)> {
        self.footer_rects(env, measurer)
            .into_iter()
            .find(|(_, rect, _)| tab_bar::rect_contains(*rect, point))
            .map(|(button, _, available)| (button, available))
    }

    fn window_button_at(&self, env: Env<'_>, point: (f32, f32)) -> Option<WindowButtonHit> {
        tab_bar::point_in_window_button_of(
            env.style,
            is_macos(),
            self.logical_width,
            bar_height(env.style),
            point,
            &SHOWN_WINDOW_BUTTONS,
        )
    }

    /// Borda de resize sob o ponto, se alguma: a regra das janelas de terminal
    /// (ADR-0027). O botão de fechar vence a borda no canto; nunca no macOS.
    fn resize_direction(
        &self,
        env: Env<'_>,
        point: (f32, f32),
    ) -> Option<winit::window::ResizeDirection> {
        if is_macos() || self.window.is_maximized() || self.window_button_at(env, point).is_some() {
            return None;
        }
        titlebar::resize_direction_at(
            point,
            self.logical_width,
            self.logical_height,
            false,
            env.config.appearance.window_controls.resize_border as f32,
        )
    }

    // ---- o formulário

    /// Mede o formulário se ele ainda não está medido para esta largura, esta
    /// cópia e este idioma, e então resolve o que esperava por ele: o foco
    /// inicial (§5) e o foco no primeiro campo inválido de um OK recusado (§6).
    fn ensure_form(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, draft: &ProfilesDraft) {
        if self.form.is_some() && self.form_width_bits == self.logical_width.to_bits() {
            return;
        }
        let m = Self::metrics(env);
        let problems = if self.show_problems {
            draft.editor_problems(&self.editor).fields
        } else {
            Default::default()
        };
        self.form = Some(FormView::build(
            self.editor.form(),
            &problems,
            &self.catalog,
            &m,
            self.logical_width,
            measurer,
        ));
        self.form_width_bits = self.logical_width.to_bits();
        let body_height = self.layout(env).panel_body.height;
        if let Some(form) = &self.form {
            self.scroll =
                layout::clamp_scroll(self.scroll, form.geometry.content_height, body_height);
        }
        if let Some(problem) = self.pending_focus.take() {
            let target = self
                .form
                .as_ref()
                .and_then(|form| problem_row(form, problem));
            if let Some((row, part)) = target {
                self.focus_row(env, row);
                if let Some(part) = part {
                    self.start_edit(row, part);
                }
            }
        } else if self.initial_focus {
            self.initial_focus = false;
            self.focus_row(env, 0);
        }
    }

    /// A copia mudou: o formulário é medido de novo no próximo uso.
    fn invalidate_form(&mut self) {
        self.form = None;
        self.window.request_redraw();
    }

    fn rows(&self) -> usize {
        self.form.as_ref().map_or(0, |form| form.rows.len())
    }

    fn row_target(&self, index: usize) -> Option<ProfileTarget> {
        self.form.as_ref()?.rows.get(index)?.profile
    }

    /// Leva o foco do teclado à linha `index` e a traz à vista. Num campo de
    /// texto, o foco já é a edição dele (§5).
    fn focus_row(&mut self, env: Env<'_>, index: usize) {
        self.focus = Some(EditorFocus::Row(index));
        self.reveal(env, index);
        let is_field = self
            .form
            .as_ref()
            .and_then(|form| form.rows.get(index))
            .is_some_and(is_text_field);
        if is_field {
            self.start_edit(index, EditPart::Field);
        }
        self.window.request_redraw();
    }

    /// A menor rolagem que deixa a linha `index` inteira à vista.
    fn reveal(&mut self, env: Env<'_>, index: usize) {
        let body_height = self.layout(env).panel_body.height;
        let Some(form) = &self.form else {
            return;
        };
        if let Some(row) = form.geometry.rows.get(index) {
            self.scroll = layout::clamp_scroll(
                layout::scroll_to_reveal(self.scroll, row.rect.y, row.rect.height, body_height),
                form.geometry.content_height,
                body_height,
            );
        }
    }

    // ---- edição

    /// O texto com que a edição de `part` na linha `index` começa: o que o campo
    /// mostra.
    fn edit_initial(&self, index: usize, part: EditPart) -> Option<String> {
        match (&self.form.as_ref()?.rows.get(index)?.control, part) {
            (ControlView::Field { text, .. }, EditPart::Field) => Some(text.clone()),
            (ControlView::List { items, .. }, EditPart::ListFirst(item)) => {
                items.get(item).map(|item| item.first.clone())
            }
            (ControlView::List { items, .. }, EditPart::ListSecond(item)) => {
                items.get(item).map(|item| item.second.clone())
            }
            _ => None,
        }
    }

    /// Começa a editar o campo da linha pelo teclado: o cursor no fim.
    fn start_edit(&mut self, index: usize, part: EditPart) {
        if let Some(initial) = self.edit_initial(index, part) {
            self.editing = Some(Editing::new(index, part, initial));
        }
    }

    /// Clique na parte de um campo: se o campo já é o que está em edição, só
    /// move o cursor (e arma o arraste); senão confirma o outro e começa a
    /// editar este, com o cursor onde se clicou.
    fn begin_or_continue_edit(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
        index: usize,
        part: EditPart,
    ) {
        let already = self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.block == index && editing.part == part);
        if !already {
            self.commit_edit(env, measurer, draft);
            self.start_edit(index, part);
        }
        if let Some(byte) = self.byte_at(env, measurer)
            && let Some(editing) = &mut self.editing
        {
            editing.state.click_at(byte);
            self.dragging_field = true;
        }
        self.window.request_redraw();
    }

    /// A posição, em bytes, do texto do campo em edição que o ponteiro aponta.
    /// Converte o x do mouse com o **mesmo** `editing_text_x` que a pintura usa.
    fn byte_at(&self, env: Env<'_>, measurer: &mut TextMeasurer) -> Option<usize> {
        let editing = self.editing.as_ref()?;
        let body = self.layout(env).panel_body;
        let rect =
            self.form
                .as_ref()?
                .field_rect(editing.block, editing.part, body, self.scroll)?;
        let m = Self::metrics(env);
        let text = editing.state.text();
        let width = measurer.measure_width(text, BODY_FONT, m.field_font_size);
        let text_x = paint::editing_text_x(&m, rect, width);
        Some(measurer.index_at_offset(
            text,
            BODY_FONT,
            m.field_font_size,
            self.cursor_logical().0 - text_x,
        ))
    }

    /// Confirma o campo em edição na **cópia**: o texto vira o do perfil. Texto
    /// que não mudou desde o começo não faz nada.
    fn commit_edit(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, draft: &ProfilesDraft) {
        self.dragging_field = false;
        let Some(editing) = self.editing.take() else {
            return;
        };
        if !editing.changed() {
            return;
        }
        let text = editing.state.text().to_owned();
        match (self.row_target(editing.block), editing.part) {
            (Some(ProfileTarget::Field(field)), EditPart::Field) => {
                self.editor.set_text(draft, field, &text);
                // As contas do cofre são o destino e o arquivo da chave: mudou um
                // deles, o que há guardado precisa ser perguntado de novo.
                if matches!(
                    field,
                    Field::Host | Field::Port | Field::User | Field::IdentityFile
                ) {
                    self.secrets_stale = true;
                }
            }
            (Some(ProfileTarget::List(kind)), EditPart::ListFirst(item)) => {
                self.editor.set_list_text(kind, item, false, &text);
            }
            (Some(ProfileTarget::List(kind)), EditPart::ListSecond(item)) => {
                self.editor.set_list_text(kind, item, true, &text);
            }
            _ => {}
        }
        // A cópia mudou -- e pode ter mudado outro campo junto (o `id` de um perfil
        // novo segue o nome). O formulário é medido de novo **agora**, não no
        // próximo quadro: quem confirma um campo costuma começar o próximo, e o
        // texto inicial deste vem do formulário.
        self.invalidate_form();
        self.ensure_form(env, measurer, draft);
    }

    /// Acrescenta um item vazio à lista da linha `index` e põe o primeiro campo
    /// dele em edição. O formulário medido ainda é o antigo: a edição nasce
    /// direto, e o item novo entra na medição seguinte.
    fn add_list_item(&mut self, index: usize, kind: ListKind) {
        if let Some(item) = self.editor.list_add(kind) {
            // Medido de novo antes de a edição ler o texto inicial do item.
            self.form = None;
            self.editing = Some(Editing::new(
                index,
                EditPart::ListFirst(item),
                String::new(),
            ));
            self.window.request_redraw();
        }
    }

    /// "Esquecer" é imediato (o segredo não é do arquivo): Cancelar não o
    /// desfaz, e esquecer o que não existe não é erro (ADR-0073 §4).
    fn forget(&self, kind: super::profiles_draft::SecretKind) {
        if let Some(target) = self.editor.forget_target(kind) {
            forget_in_vault(&target);
        }
    }

    // ---- a lista de escolha

    /// Abre a lista da autenticação, ancorada ao controle da linha `index`.
    fn open_auth_choice(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, index: usize) {
        let body = self.layout(env).panel_body;
        let Some(anchor) = self
            .form
            .as_ref()
            .and_then(|form| form.control_rect(index, body, self.scroll))
        else {
            return;
        };
        let (items, current) = content::auth_choice(
            self.editor.form().auth_preset(),
            &self.catalog,
            env.config,
            anchor.width,
            measurer,
        );
        self.choice = Some(ChoiceList::open(index, items, &current));
        self.window.request_redraw();
    }

    fn choice_layout(&self, env: Env<'_>) -> Option<ChoiceLayout> {
        let list = self.choice.as_ref()?;
        let body = self.layout(env).panel_body;
        let anchor = self
            .form
            .as_ref()?
            .control_rect(list.block, body, self.scroll)?;
        Some(choice_list::layout(
            list,
            anchor,
            env.config,
            self.logical_width,
            self.logical_height,
        ))
    }

    /// Escolhe o item realçado da lista e a fecha: o conjunto de métodos vai à
    /// cópia.
    fn select_choice(&mut self) {
        let Some(list) = self.choice.take() else {
            return;
        };
        if let Some(Ok(preset)) = list.highlighted_value().map(str::parse::<usize>) {
            self.editor.set_auth_preset(preset);
            self.invalidate_form();
        }
        self.window.request_redraw();
    }

    // ---- ponteiro

    pub(crate) fn cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) {
        self.cursor_position = (position.x, position.y);
        self.ensure_form(env, measurer, draft);
        let point = self.cursor_logical();
        if self.dragging_field {
            if let Some(byte) = self.byte_at(env, measurer)
                && let Some(editing) = &mut self.editing
            {
                editing.state.drag_to(byte);
                self.window.request_redraw();
            }
            return;
        }
        // A lista aberta cobre o resto: só ela reage ao ponteiro.
        if self.choice.is_some() {
            let before = self.choice.as_ref().map(|list| list.highlighted);
            if let (Some(layout), Some(list)) = (self.choice_layout(env), self.choice.as_mut())
                && let Some(index) = choice_list::hit(&layout, point)
            {
                list.highlighted = index;
            }
            self.window.set_cursor(CursorIcon::Default);
            if before != self.choice.as_ref().map(|list| list.highlighted) {
                self.window.request_redraw();
            }
            return;
        }
        let hovered_window_button = if is_macos() {
            None
        } else {
            self.window_button_at(env, point)
        };
        // Um botão desabilitado não reage ao ponteiro.
        let hovered_button = self
            .button_at(env, measurer, point)
            .and_then(|(button, available)| available.then_some(button));
        let body = self.layout(env).panel_body;
        let m = Self::metrics(env);
        let hovered = self
            .form
            .as_ref()
            .and_then(|form| form.hit(&m, body, self.scroll, point));
        self.window
            .set_cursor(match self.resize_direction(env, point) {
                Some(direction) => CursorIcon::from(direction),
                None => self.cursor_for(
                    hovered,
                    hovered_button.is_some() || hovered_window_button.is_some(),
                ),
            });
        let before = (
            self.hovered,
            self.hovered_button,
            self.hovered_window_button,
        );
        self.hovered = hovered;
        self.hovered_button = hovered_button;
        self.hovered_window_button = hovered_window_button;
        if before
            != (
                self.hovered,
                self.hovered_button,
                self.hovered_window_button,
            )
        {
            self.window.request_redraw();
        }
    }

    /// A forma do cursor: I sobre um campo de texto, mão sobre o que se clica.
    fn cursor_for(&self, hit: Option<Hit>, over_button: bool) -> CursorIcon {
        match hit {
            Some(Hit::Control(index, part)) => {
                match (
                    self.form.as_ref().and_then(|form| form.rows.get(index)),
                    part,
                ) {
                    (Some(row), ControlPart::Whole) if is_text_field(row) => CursorIcon::Text,
                    (_, ControlPart::ListField { .. }) => CursorIcon::Text,
                    _ => CursorIcon::Pointer,
                }
            }
            _ if over_button => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        }
    }

    pub(crate) fn cursor_left(&mut self) {
        let changed = self.hovered.take().is_some()
            || self.hovered_button.take().is_some()
            || self.hovered_window_button.take().is_some();
        if changed {
            self.window.request_redraw();
        }
    }

    /// Roda do mouse sobre o corpo: rola na vertical, limitada ao fim do
    /// conteúdo (o modelo de rolagem da tela).
    pub(crate) fn wheel(
        &mut self,
        delta: MouseScrollDelta,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) {
        self.ensure_form(env, measurer, draft);
        let body = self.layout(env).panel_body;
        if !tab_bar::rect_contains(body, self.cursor_logical()) {
            return;
        }
        let pixels = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines * env.style.overflow_scroll_step,
            MouseScrollDelta::PixelDelta(position) => position.y as f32 / self.scale,
        };
        let content_height = self
            .form
            .as_ref()
            .map_or(0.0, |form| form.geometry.content_height);
        self.scroll = layout::clamp_scroll(self.scroll - pixels, content_height, body.height);
        // A lista é ancorada à linha: rolar a deixaria solta.
        self.choice = None;
        self.window.request_redraw();
    }

    /// Clique esquerdo (ADR-0027): botão de janela (arma), borda de resize,
    /// drag region do cabeçalho, a lista aberta, um botão do rodapé, ou o que o
    /// formulário tem sob o ponto.
    pub(crate) fn left_pressed(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) -> EditorPress {
        self.ensure_form(env, measurer, draft);
        let point = self.cursor_logical();
        if !is_macos() {
            if let Some(hit) = self.window_button_at(env, point) {
                // Só arma: dispara no release (`left_released`).
                self.pressed_window_button = Some(hit);
                self.window.request_redraw();
                return EditorPress::Nothing;
            }
            if let Some(direction) = self.resize_direction(env, point) {
                let _ = self.window.drag_resize_window(direction);
                return EditorPress::Nothing;
            }
            if point.1 < header_height(env.style) {
                // Sem maximizar (ADR-0080 §3): o duplo clique não faz nada.
                titlebar_drag(&self.window, &mut self.last_titlebar_click, false);
                return EditorPress::Nothing;
            }
        }
        // A lista aberta é modal: um clique escolhe um item ou a fecha, e em
        // nenhum dos dois casos chega ao que está por baixo.
        if self.choice.is_some() {
            self.arm_choice(env, point);
            return EditorPress::Nothing;
        }
        if let Some((button, available)) = self.button_at(env, measurer, point) {
            // Um "Esquecer" desabilitado não faz nada, nem leva o foco.
            if !available {
                return EditorPress::Nothing;
            }
            // O foco vai no press; a ação, no release (ADR-0081).
            self.commit_edit(env, measurer, draft);
            self.focus = Some(EditorFocus::Button(button));
            self.armed = Some(Armed::Button(button));
            self.window.request_redraw();
            return EditorPress::Nothing;
        }
        let hit = self.form_hit(env, point);
        match hit {
            // Campo de texto: foco e arraste de seleção já no press. Botão de
            // linha: arma e age no release.
            Some(Hit::Control(index, part)) if self.defers(index, part) => {
                self.armed = Some(Armed::Control(index, part));
                self.window.request_redraw();
            }
            Some(Hit::Control(index, part)) => {
                self.click_control(env, measurer, draft, index, part);
            }
            Some(Hit::Row(index)) => {
                self.commit_edit(env, measurer, draft);
                self.focus_row(env, index);
            }
            _ => self.commit_edit(env, measurer, draft),
        }
        EditorPress::Nothing
    }

    /// O clique numa parte do controle de uma linha: o que o painel do grupo
    /// faz (`content::profile_click`), só que sobre a cópia.
    fn click_control(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
        index: usize,
        part: ControlPart,
    ) {
        let Some(target) = self.row_target(index) else {
            return;
        };
        match content::profile_click(target, part) {
            ProfileClick::Edit(edit) => {
                self.focus = Some(EditorFocus::Row(index));
                self.begin_or_continue_edit(env, measurer, draft, index, edit);
            }
            ProfileClick::OpenAuth => {
                self.commit_edit(env, measurer, draft);
                self.focus = Some(EditorFocus::Row(index));
                self.open_auth_choice(env, measurer, index);
            }
            ProfileClick::ListRemove(kind, item) => {
                self.commit_edit(env, measurer, draft);
                self.focus = Some(EditorFocus::Row(index));
                if self.editor.list_remove(kind, item) {
                    self.invalidate_form();
                }
            }
            ProfileClick::ListAdd(kind) => {
                self.commit_edit(env, measurer, draft);
                self.focus = Some(EditorFocus::Row(index));
                self.add_list_item(index, kind);
            }
            // As outras ações são da lista de perfis do painel do grupo.
            _ => {}
        }
    }

    /// O alvo do formulário sob `point`.
    fn form_hit(&self, env: Env<'_>, point: (f32, f32)) -> Option<Hit> {
        let body = self.layout(env).panel_body;
        let m = Self::metrics(env);
        self.form
            .as_ref()
            .and_then(|form| form.hit(&m, body, self.scroll, point))
    }

    /// O clique nesta parte de controle só age no release (ADR-0081)? O campo
    /// de texto não: o press leva o foco e começa o arraste de seleção.
    fn defers(&self, index: usize, part: ControlPart) -> bool {
        self.row_target(index).is_some_and(|target| {
            !matches!(content::profile_click(target, part), ProfileClick::Edit(_))
        })
    }

    /// Press com a lista aberta: arma o item sob o ponteiro; fora do menu a
    /// fecha já no press.
    fn arm_choice(&mut self, env: Env<'_>, point: (f32, f32)) {
        let Some(layout) = self.choice_layout(env) else {
            self.choice = None;
            return;
        };
        if let Some(index) = choice_list::hit(&layout, point) {
            self.armed = Some(Armed::ChoiceItem(index));
        } else if !choice_list::contains(&layout, point) {
            self.choice = None;
        }
        self.window.request_redraw();
    }

    /// O release de um alvo armado: age se o cursor ainda está sobre ele.
    fn release_armed(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
        armed: Armed,
    ) -> EditorPress {
        let point = self.cursor_logical();
        let over = match armed {
            Armed::Button(_) => self
                .button_at(env, measurer, point)
                .filter(|(_, available)| *available)
                .map(|(button, _)| Armed::Button(button)),
            Armed::Control(..) => match self.form_hit(env, point) {
                Some(Hit::Control(index, part)) => Some(Armed::Control(index, part)),
                _ => None,
            },
            Armed::ChoiceItem(_) => self
                .choice_layout(env)
                .and_then(|layout| choice_list::hit(&layout, point))
                .map(Armed::ChoiceItem),
        };
        match crate::input::release_on_armed(Some(armed), over) {
            Some(Armed::Button(button)) => {
                self.window.request_redraw();
                self.press_button(button)
            }
            Some(Armed::Control(index, part)) => {
                self.click_control(env, measurer, draft, index, part);
                EditorPress::Nothing
            }
            Some(Armed::ChoiceItem(index)) => {
                if let Some(list) = &mut self.choice {
                    list.highlighted = index;
                }
                self.select_choice();
                EditorPress::Nothing
            }
            None => EditorPress::Nothing,
        }
    }

    /// O botão esquerdo foi solto: acaba o arraste de seleção, age no alvo
    /// armado se o cursor ainda está sobre ele (ADR-0081) e dispara o botão de
    /// janela armado do mesmo jeito. Fechar é Cancelar.
    pub(crate) fn left_released(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) -> EditorPress {
        self.dragging_field = false;
        if let Some(armed) = self.armed.take() {
            self.window.request_redraw();
            return self.release_armed(env, measurer, draft, armed);
        }
        let armed = self.pressed_window_button.take();
        if armed.is_none() || is_macos() {
            return EditorPress::Nothing;
        }
        self.window.request_redraw();
        let released_over = self.window_button_at(env, self.cursor_logical());
        match tab_bar::window_button_release(armed, released_over) {
            Some(WindowButtonHit::Close) => EditorPress::Cancel,
            _ => EditorPress::Nothing,
        }
    }

    /// A janela perdeu o foco: um press armado não recebe mais o release.
    pub(crate) fn focus_lost(&mut self) {
        self.dragging_field = false;
        let window_button = self.pressed_window_button.take().is_some();
        let armed = self.armed.take().is_some();
        if window_button || armed {
            self.window.request_redraw();
        }
    }

    fn press(button: EditorButton) -> EditorPress {
        match button {
            EditorButton::Cancel => EditorPress::Cancel,
            EditorButton::Ok => EditorPress::Ok,
            // Os "Esquecer" agem na hora, dentro da janela: não pedem nada a ela.
            EditorButton::ForgetPassword | EditorButton::ForgetPassphrase => EditorPress::Nothing,
        }
    }

    /// Aciona um botão do rodapé: Cancelar e OK pedem à tela; um "Esquecer"
    /// apaga o segredo do cofre **na hora** (Cancelar não o traz de volta,
    /// ADR-0073 §4), pergunta de novo o que sobrou -- o botão fica
    /// desabilitado -- e, se estava com o foco, o foco vai para Cancelar.
    fn press_button(&mut self, button: EditorButton) -> EditorPress {
        let Some(kind) = button.secret() else {
            return Self::press(button);
        };
        self.forget(kind);
        self.secrets_stale = true;
        self.sync_secrets();
        let still_there = self
            .footer_state()
            .into_iter()
            .any(|(candidate, available)| candidate == button && available);
        if self.focus == Some(EditorFocus::Button(button)) && !still_there {
            self.focus = Some(EditorFocus::Button(EditorButton::Cancel));
        }
        self.window.request_redraw();
        EditorPress::Nothing
    }

    // ---- teclado

    pub(crate) fn modifiers_changed(&mut self, state: ModifiersState) {
        self.modifiers = modifiers_from(state);
    }

    /// `Ctrl` no Windows e no Linux, `Cmd` no macOS.
    fn primary(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.modifiers.super_
        } else {
            self.modifiers.ctrl
        }
    }

    /// Teclas do editor (ADR-0080 §5). Com a lista aberta, só ela as lê. `Tab`
    /// e `Shift+Tab` percorrem as linhas e depois os botões; `Enter` num campo
    /// de uma linha é OK; `Esc` cancela quando nenhum campo ou lista o consome.
    /// `Ctrl+S`/`Cmd+S` **não faz nada**: salvar é da tela, e a tela só se
    /// alcança depois de fechar o editor.
    pub(crate) fn key(
        &mut self,
        event: &KeyEvent,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) -> EditorPress {
        if event.state != ElementState::Pressed {
            return EditorPress::Nothing;
        }
        self.ensure_form(env, measurer, draft);
        if self.choice.is_some() {
            self.key_choice(&event.logical_key);
            return EditorPress::Nothing;
        }
        let is_save = matches!(
            &event.logical_key,
            Key::Character(text) if text.eq_ignore_ascii_case("s")
        );
        if self.primary() && is_save {
            return EditorPress::Nothing;
        }
        if self.editing.is_some() {
            return self.key_editing(event, env, measurer, draft);
        }
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => EditorPress::Cancel,
            Key::Named(NamedKey::Tab) => {
                self.move_focus(env, measurer, draft);
                EditorPress::Nothing
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => self.activate(env, measurer),
            _ => EditorPress::Nothing,
        }
    }

    /// Teclas com um campo em edição: `Enter` confirma (e num campo de uma
    /// linha é OK), `Esc` desfaz a digitação ou, sem ela, cancela, `Tab`
    /// confirma e segue, e o resto edita o texto.
    fn key_editing(
        &mut self,
        event: &KeyEvent,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) -> EditorPress {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                let changed = self.editing.as_ref().is_some_and(Editing::changed);
                match esc_action(changed) {
                    EscAction::Cancel => return EditorPress::Cancel,
                    EscAction::RevertTyping => {
                        if let Some(editing) = self.editing.take() {
                            self.editing =
                                Some(Editing::new(editing.block, editing.part, editing.initial));
                        }
                        self.dragging_field = false;
                    }
                }
            }
            Key::Named(NamedKey::Enter) => {
                let target = self
                    .editing
                    .as_ref()
                    .and_then(|editing| self.row_target(editing.block));
                self.commit_edit(env, measurer, draft);
                if enter_is_ok(target) {
                    return EditorPress::Ok;
                }
            }
            Key::Named(NamedKey::Tab) => {
                // Numa lista, `Tab` anda pelos campos dos itens; nas pontas
                // dela sai como de qualquer campo.
                if !self.walk_list_field(env, measurer, draft, self.modifiers.shift) {
                    self.move_focus(env, measurer, draft);
                }
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(editing) = &mut self.editing {
                    editing.state.backspace();
                }
            }
            key => {
                if let Some(editing) = &mut self.editing {
                    apply_text_field_key(
                        &mut editing.state,
                        key,
                        event.text.as_deref(),
                        self.modifiers,
                    );
                }
            }
        }
        self.window.request_redraw();
        EditorPress::Nothing
    }

    /// `Tab`/`Shift+Tab` num campo de lista: confirma o campo e passa para o
    /// próximo -- nome, valor, próximo item. Devolve `false` nas pontas da
    /// lista, ou fora de uma, e quem chama segue como de qualquer campo.
    fn walk_list_field(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
        backwards: bool,
    ) -> bool {
        let Some(editing) = &self.editing else {
            return false;
        };
        let block = editing.block;
        let Some(ControlView::List {
            items, two_fields, ..
        }) = self
            .form
            .as_ref()
            .and_then(|form| form.rows.get(block))
            .map(|row| &row.control)
        else {
            return false;
        };
        let Some(next) = editing
            .part
            .next_in_list(items.len(), *two_fields, backwards)
        else {
            return false;
        };
        self.commit_edit(env, measurer, draft);
        self.start_edit(block, next);
        true
    }

    /// Passa o foco ao próximo ponto de parada -- as linhas, depois os botões
    /// -- e leva a linha à vista. Confirma antes o campo em edição.
    fn move_focus(&mut self, env: Env<'_>, measurer: &mut TextMeasurer, draft: &ProfilesDraft) {
        self.commit_edit(env, measurer, draft);
        let buttons = self.footer_state();
        let order = focus_order(self.rows(), &buttons);
        let next = next_in_order(&order, self.focus, self.modifiers.shift);
        match next {
            EditorFocus::Row(index) => self.focus_row(env, index),
            EditorFocus::Button(_) => self.focus = Some(next),
        }
        self.window.request_redraw();
    }

    /// `Enter`/`Espaço` no que tem o foco, sem campo em edição: o botão do
    /// rodapé (um "Esquecer" esquece o segredo), ou a ação da linha -- abre a
    /// escolha, entra na lista.
    fn activate(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) -> EditorPress {
        match self.focus {
            Some(EditorFocus::Button(button)) => self.press_button(button),
            Some(EditorFocus::Row(index)) => {
                match self.row_target(index) {
                    Some(ProfileTarget::Field(_)) => self.start_edit(index, EditPart::Field),
                    Some(ProfileTarget::Auth) => self.open_auth_choice(env, measurer, index),
                    Some(ProfileTarget::List(kind)) => {
                        let empty = self.editor.list_rows(kind).is_empty();
                        if empty {
                            self.add_list_item(index, kind);
                        } else {
                            self.start_edit(index, EditPart::ListFirst(0));
                        }
                    }
                    _ => {}
                }
                self.window.request_redraw();
                EditorPress::Nothing
            }
            None => EditorPress::Nothing,
        }
    }

    /// Teclas com a lista aberta: setas movem o realce, `Enter` e `Espaço`
    /// escolhem, `Esc` fecha **só a lista**. O resto é engolido.
    fn key_choice(&mut self, key: &Key) {
        match key {
            Key::Named(NamedKey::Escape) => self.choice = None,
            Key::Named(NamedKey::ArrowUp) => {
                if let Some(list) = &mut self.choice {
                    list.move_highlight(-1);
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if let Some(list) = &mut self.choice {
                    list.move_highlight(1);
                }
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => self.select_choice(),
            _ => {}
        }
        self.window.request_redraw();
    }

    // ---- acessibilidade

    /// Monta a árvore de acessibilidade e a entrega -- só se houver cliente
    /// conectado, e nunca pede quadro (ADR-0043 §3). Projeção do mesmo layout e
    /// do mesmo formulário que a pintura lê.
    pub(crate) fn refresh_access_tree(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        language: &str,
        draft: &ProfilesDraft,
    ) {
        self.ensure_form(env, measurer, draft);
        let layout = self.layout(env);
        let title = self.title_text();
        let focus = self.focus;
        let footer = self.footer_state();
        let catalog = &self.catalog;
        let rows: &[_] = self.form.as_ref().map_or(&[], |form| &form.rows);
        self.access_adapter.update_if_active(|| {
            access::build_profile_editor_tree(
                &layout, &title, rows, &footer, focus, catalog, language,
            )
        });
    }

    // ---- pintura

    /// Pinta o quadro: o formulário em coluna ao fundo, o rodapé e -- fora do
    /// macOS -- o cabeçalho por cima, tudo na camada `Chrome` (ADR-0060 §4); a
    /// lista de escolha na `Popover`; a borda de 1px da janela na mais alta.
    pub(crate) fn paint(
        &mut self,
        env: Env<'_>,
        pal: &ResolvedPalette,
        term_pal: &ResolvedTermPalette,
        measurer: &mut TextMeasurer,
        draft: &ProfilesDraft,
    ) -> Frame {
        self.ensure_form(env, measurer, draft);
        let m = Self::metrics(env);
        let layout = self.layout(env);
        let rects = self.footer_rects(env, measurer);
        let mut out = vec![quad(layout.panel, pal.settings_panel_background)];
        if let Some(form) = &self.form {
            let ctx = ControlCtx {
                metrics: &m,
                hovered: self.hovered,
                editing: self.editing.as_ref(),
                selection_color: term_pal.selection_background,
                read_only: false,
                style: env.style,
                pal,
                config: env.config,
            };
            // O campo em edição já tem o anel de Acento: o anel de foco é das
            // linhas que não estão sendo editadas.
            let focused = match self.focus {
                Some(EditorFocus::Row(index))
                    if self
                        .editing
                        .as_ref()
                        .is_none_or(|editing| editing.block != index) =>
                {
                    Some(index)
                }
                _ => None,
            };
            form.paint(
                &ctx,
                layout.panel_body,
                self.scroll,
                focused,
                measurer,
                &mut out,
            );
        }
        out.push(quad(
            Rect {
                height: 1.0_f32.min(layout.footer.height),
                ..layout.footer
            },
            pal.settings_footer_separator,
        ));
        for (button, rect, available) in rects {
            let label = button.label(&self.catalog);
            paint_button(
                &FooterButtonView {
                    rect,
                    label: &label,
                    primary: button == EditorButton::Ok,
                    available,
                    hovered: self.hovered_button == Some(button),
                    pressed: self.hovered_button == Some(button)
                        && self.armed == Some(Armed::Button(button)),
                    focused: self.focus == Some(EditorFocus::Button(button)),
                },
                &m,
                pal,
                env.style,
                &mut out,
            );
        }
        if let Some(header) = layout.header {
            out.push(quad(header, pal.bar_background));
            paint_header_title(header, env.style, pal, None, &self.title_text(), &mut out);
            chrome::paint_window_buttons_of(
                env.style,
                pal,
                self.logical_width,
                header.height,
                false,
                self.hovered_window_button,
                self.pressed_window_button,
                &SHOWN_WINDOW_BUTTONS,
                &mut out,
            );
        }
        let mut frame = Frame::new();
        frame.set_layer(Layer::Chrome, out);
        if let (Some(list), Some(list_layout)) = (&self.choice, self.choice_layout(env)) {
            frame.set_layer(
                Layer::Popover,
                choice_list::paint(list, &list_layout, env.config, pal),
            );
        }
        for primitive in chrome::window_border(
            self.logical_width,
            self.logical_height,
            pal.settings_panel_background,
        ) {
            frame.push(Layer::Modal, primitive);
        }
        frame
    }

    /// Submete o quadro. O fundo de limpeza é o do painel, que ocupa a maior
    /// parte da janela.
    pub(crate) fn render(&mut self, gpu: &mut GpuContext, pal: &ResolvedPalette, frame: &Frame) {
        self.surface
            .render(gpu, pal.settings_panel_background, frame, 1.0);
        if self.settle_frames > 0 {
            self.settle_frames -= 1;
            self.window.request_redraw();
        }
    }
}

fn quad(rect: Rect, color: porecatu_render::Color) -> Primitive {
    Primitive::Quad(Quad { rect, color })
}

/// Uma faixa do tamanho de abertura do editor, para os testes de acessibilidade
/// e de layout: a mesma função de faixas da janela.
#[cfg(test)]
pub(crate) fn layout_for_test(with_header: bool) -> Layout {
    let settings = porecatu_config::Config::default().appearance.settings;
    let m = Metrics::from_config(
        &porecatu_config::Config::default(),
        if with_header { 52.0 } else { 0.0 },
    );
    layout::layout_with_banner(
        settings.profile_window_width as f32,
        settings.profile_window_height as f32,
        m.header_height,
        0.0,
        m.footer_height(),
        0.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab_bar::TabBarStyle;

    #[test]
    fn tab_walks_the_rows_then_the_buttons_and_wraps_around() {
        let order = focus_order(3, &footer_buttons(false, false, Secrets::default()));
        assert_eq!(
            order,
            [
                EditorFocus::Row(0),
                EditorFocus::Row(1),
                EditorFocus::Row(2),
                EditorFocus::Button(EditorButton::Cancel),
                EditorFocus::Button(EditorButton::Ok),
            ]
        );
        let mut at = Some(EditorFocus::Row(0));
        let mut walked = Vec::new();
        for _ in 0..order.len() {
            let next = next_in_order(&order, at, false);
            walked.push(next);
            at = Some(next);
        }
        // Dá a volta: o último é o primeiro de novo.
        assert_eq!(walked[..4], order[1..]);
        assert_eq!(walked[4], EditorFocus::Row(0));
    }

    #[test]
    fn shift_tab_walks_backwards_and_wraps_to_the_last_button() {
        let order = focus_order(2, &footer_buttons(false, false, Secrets::default()));
        assert_eq!(
            next_in_order(&order, Some(EditorFocus::Row(0)), true),
            EditorFocus::Button(EditorButton::Ok),
            "before the first field comes the last button"
        );
        assert_eq!(
            next_in_order(
                &order,
                Some(EditorFocus::Button(EditorButton::Cancel)),
                true
            ),
            EditorFocus::Row(1)
        );
        // Sem foco, ou com um que saiu da ordem: o primeiro, ou o último.
        assert_eq!(next_in_order(&order, None, false), EditorFocus::Row(0));
        assert_eq!(
            next_in_order(&order, None, true),
            EditorFocus::Button(EditorButton::Ok)
        );
        assert_eq!(
            next_in_order(&order, Some(EditorFocus::Row(9)), false),
            EditorFocus::Row(0)
        );
    }

    #[test]
    fn a_form_with_no_rows_still_tabs_between_the_two_buttons() {
        let order = focus_order(0, &footer_buttons(false, false, Secrets::default()));
        assert_eq!(
            next_in_order(&order, None, false),
            EditorFocus::Button(EditorButton::Cancel)
        );
        assert_eq!(
            next_in_order(
                &order,
                Some(EditorFocus::Button(EditorButton::Cancel)),
                false
            ),
            EditorFocus::Button(EditorButton::Ok)
        );
    }

    #[test]
    fn esc_in_a_field_reverts_the_typing_and_without_typing_cancels() {
        assert_eq!(esc_action(true), EscAction::RevertTyping);
        assert_eq!(esc_action(false), EscAction::Cancel);
    }

    #[test]
    fn enter_is_ok_in_a_one_line_field_and_only_confirms_in_a_list() {
        assert!(enter_is_ok(Some(ProfileTarget::Field(Field::Name))));
        assert!(enter_is_ok(Some(ProfileTarget::Field(Field::Port))));
        assert!(!enter_is_ok(Some(ProfileTarget::List(ListKind::Args))));
        assert!(!enter_is_ok(Some(ProfileTarget::List(ListKind::Env))));
        assert!(!enter_is_ok(Some(ProfileTarget::Auth)));
        assert!(!enter_is_ok(None));
    }

    #[test]
    fn a_refused_ok_points_the_focus_at_the_row_of_the_first_invalid_field() {
        use super::super::profile_form::{fixture_draft, fixture_view};
        // SSH: o host inválido. O foco vai à linha dele, e é a que tem a nota.
        let draft = fixture_draft();
        let mut editor = draft.open_editor(1).unwrap();
        editor.set_text(&draft, Field::Host, "a b");
        let problems = draft.editor_problems(&editor);
        let first = problems.first.expect("a first invalid field");
        let view = fixture_view(true, true, 560.0);
        let (row, part) = problem_row(&view, first).expect("the row");
        assert_eq!(
            view.rows[row].profile,
            Some(ProfileTarget::Field(Field::Host))
        );
        assert_eq!(part, None, "a text field is focused whole");
        assert!(view.rows[row].invalid.is_some(), "and it carries the note");

        // Vários inválidos: o primeiro na ordem do formulário, não na do mapa.
        editor.set_text(&draft, Field::Name, "");
        let first = draft.editor_problems(&editor).first.unwrap();
        let view = fixture_view(true, false, 560.0);
        let (row, _) = problem_row(&view, first).unwrap();
        assert_eq!(
            view.rows[row].profile,
            Some(ProfileTarget::Field(Field::Name)),
            "the name comes before the host"
        );
    }

    #[test]
    fn a_refused_environment_item_focuses_the_name_of_that_item() {
        use super::super::profile_form::{fixture_draft, fixture_view};
        let draft = fixture_draft();
        let mut editor = draft.open_editor(0).unwrap();
        let item = editor.list_add(ListKind::Env).unwrap();
        editor.set_list_text(ListKind::Env, item, true, "value-without-name");
        let first = draft.editor_problems(&editor).first.unwrap();
        assert_eq!(first, ProblemFocus::EnvItem(item));
        let view = fixture_view(false, false, 560.0);
        assert_eq!(
            problem_row(&view, first),
            Some((
                view.row_of(ProfileTarget::List(ListKind::Env)).unwrap(),
                Some(EditPart::ListFirst(item))
            ))
        );
    }

    #[test]
    fn the_authentication_problem_points_at_its_choice_row() {
        use super::super::profile_form::fixture_view;
        let view = fixture_view(true, false, 560.0);
        let (row, part) = problem_row(&view, ProblemFocus::Field(Field::Auth)).unwrap();
        assert_eq!(view.rows[row].profile, Some(ProfileTarget::Auth));
        assert_eq!(part, None);
        // O tipo local não tem a escolha: nada a focar.
        let local = fixture_view(false, false, 560.0);
        assert_eq!(problem_row(&local, ProblemFocus::Field(Field::Auth)), None);
    }

    #[test]
    fn forgetting_acts_on_the_copy_as_it_stands_and_never_waits_for_ok() {
        use super::super::profile_form::fixture_draft;
        use super::super::profiles_draft::{Forget, SecretKind};
        let draft = fixture_draft();
        let mut editor = draft.open_editor(1).unwrap();
        // O destino que a cópia tem agora, não o do rascunho.
        editor.set_text(&draft, Field::Host, "10.9.9.9");
        editor.set_text(&draft, Field::User, "ops");
        let Some(Forget::Password(destination)) = editor.forget_target(SecretKind::Password) else {
            panic!("an SSH copy has a password to forget");
        };
        assert_eq!(
            destination,
            porecatu_term::Destination::new("ops", "10.9.9.9", 22)
        );
        assert!(matches!(
            draft.forget_target(1, SecretKind::Password),
            Some(Forget::Password(original))
                if original == porecatu_term::Destination::new("deploy", "10.4.1.20", 22)
        ));
        assert!(
            !draft.is_dirty(),
            "forgetting is immediate and is no pending change"
        );
        // Um perfil local não tem cofre.
        assert_eq!(
            draft
                .open_editor(0)
                .unwrap()
                .forget_target(SecretKind::Password),
            None
        );
    }

    #[test]
    fn the_forget_buttons_exist_only_in_an_ssh_profile_with_a_vault() {
        let have = Secrets {
            password: true,
            passphrase: false,
        };
        // SSH com cofre: os dois "Esquecer" à esquerda, depois Cancelar e OK.
        assert_eq!(
            footer_buttons(true, true, have),
            [
                (EditorButton::ForgetPassword, true),
                (EditorButton::ForgetPassphrase, false),
                (EditorButton::Cancel, true),
                (EditorButton::Ok, true),
            ]
        );
        // Sem cofre, ou num perfil local, só Cancelar e OK.
        let plain = [(EditorButton::Cancel, true), (EditorButton::Ok, true)];
        assert_eq!(footer_buttons(true, false, have), plain);
        assert_eq!(footer_buttons(false, true, have), plain);
    }

    #[test]
    fn a_forget_button_without_a_saved_secret_is_disabled_and_out_of_the_tab_order() {
        let none = footer_buttons(true, true, Secrets::default());
        assert!(
            none[..2].iter().all(|(_, available)| !available),
            "nothing saved: both are disabled, not absent"
        );
        assert_eq!(
            focus_order(1, &none),
            [
                EditorFocus::Row(0),
                EditorFocus::Button(EditorButton::Cancel),
                EditorFocus::Button(EditorButton::Ok),
            ]
        );
        // Com a senha guardada, só o da senha entra, antes de Cancelar.
        let password = footer_buttons(
            true,
            true,
            Secrets {
                password: true,
                passphrase: false,
            },
        );
        assert_eq!(
            focus_order(1, &password),
            [
                EditorFocus::Row(0),
                EditorFocus::Button(EditorButton::ForgetPassword),
                EditorFocus::Button(EditorButton::Cancel),
                EditorFocus::Button(EditorButton::Ok),
            ]
        );
    }

    #[test]
    fn the_forget_buttons_sit_at_the_left_and_cancel_and_ok_at_the_right() {
        let config = porecatu_config::Config::default();
        let m = Metrics::from_config(&config, 52.0);
        let footer = layout_for_test(true).footer;
        let [password, passphrase] = left_aligned_buttons(&m, footer, [90.0, 110.0]);
        let [cancel, ok] = right_aligned_buttons(&m, footer, [70.0, 40.0]);
        assert_eq!(
            password.x,
            footer.x + m.panel_padding,
            "flush with the padding"
        );
        assert_eq!(passphrase.x, password.x + password.width + m.button_gap);
        assert_eq!((password.y, passphrase.y), (cancel.y, ok.y));
        assert!(
            passphrase.x + passphrase.width < cancel.x,
            "the two groups do not meet"
        );
    }

    #[test]
    fn only_the_close_button_answers_in_the_header() {
        let style = TabBarStyle::DEFAULT;
        let (width, height) = (560.0, bar_height(&style));
        let at = |index: u8| {
            let rect = tab_bar::window_button_rect(&style, index, width, height);
            tab_bar::point_in_window_button_of(
                &style,
                false,
                width,
                height,
                (rect.x + 1.0, rect.y + 1.0),
                &SHOWN_WINDOW_BUTTONS,
            )
        };
        assert_eq!(at(0), None, "minimize is not there");
        assert_eq!(at(1), None, "maximize is not there");
        assert_eq!(at(2), Some(WindowButtonHit::Close));
    }

    #[test]
    fn the_footer_is_cancel_then_ok_pushed_to_the_right_edge() {
        let config = porecatu_config::Config::default();
        let m = Metrics::from_config(&config, 52.0);
        let layout = layout_for_test(true);
        let [cancel, ok] = right_aligned_buttons(&m, layout.footer, [70.0, 40.0]);
        let right_edge = layout.footer.x + layout.footer.width - m.panel_padding;
        assert_eq!(ok.x + ok.width, right_edge);
        assert_eq!(cancel.x + cancel.width + m.button_gap, ok.x);
        assert_eq!((cancel.width, ok.width), (70.0, 40.0));
        assert_eq!(cancel.y, ok.y);
    }

    #[test]
    fn the_editor_window_opens_larger_than_its_minimum() {
        let s = porecatu_config::Config::default().appearance.settings;
        assert!(s.profile_window_width >= s.profile_min_width);
        assert!(s.profile_window_height >= s.profile_min_height);
    }
}
