// SPDX-License-Identifier: GPL-3.0-or-later

//! Diálogo de confirmação (ADR-0014, PRD-010 RF-10.18): modal por janela --
//! `App` carrega no máximo um por `WindowState`, o que já implementa "modal
//! é por janela, não por app" (ADR-0014, mitigação de risco). Foco inicial
//! no cancelar; `Enter` aciona o botão focado, `Esc` sempre cancela.
//! `action` é um enum fechado, não uma closure: o diálogo é dado puro, sem
//! capturar estado de `App` dentro dele.

use std::path::PathBuf;

use porecatu_core::{GroupId, PaneId, TabId};
use porecatu_locale::Catalog;
use porecatu_term::{PromptId, SecretLabel};

use crate::messages::msg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButton {
    Cancel,
    /// O botão do meio do diálogo de três saídas (RF-16.4); os de dois
    /// botões nunca o têm.
    Discard,
    Confirm,
}

/// O que confirmar faz de verdade -- resolvido por `lib.rs`, que já tem
/// acesso ao `WindowState`/`App` que o diálogo não carrega. Os quatro
/// primeiros diálogos do v1 (RF-10.19) são todos de fechamento -- o
/// prefixo comum não é redundância, é coincidência de escopo. As duas
/// últimas variantes (RF-14.5/RF-14.16, ADR-0055 §3) carregam o dado que
/// falta pra confirmar de verdade -- nome pro sobrescrever, caminho pro
/// excluir -- e é isso que tira `Copy` do enum (`PathBuf`/`String` não
/// são `Copy`); todo call site que fazia `dialog.action` (cópia) passou a
/// `dialog.action.clone()`.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAction {
    /// RF-1.6 (ADR-0017): fechar aba com tela alternativa ou reporte de
    /// mouse ligado.
    CloseTab(TabId),
    /// RF-6.10/RF-1.6: fechar um painel (não o último da aba) com tela
    /// alternativa ou reporte de mouse ligado.
    ClosePane(TabId, PaneId),
    /// RF-10.23 (ADR-0015): fechar janela com mais de uma aba.
    CloseWindow,
    /// RF-2.22/RF-2.23 (`group.close_all`): fecha todas as abas do grupo.
    /// Confirmação **sempre**, não configurável -- "a ação mais destrutiva
    /// da interface" (ADR-0023).
    CloseGroup(GroupId),
    /// RF-14.5 (ADR-0054 §5, ADR-0055 §3): nome que já existe na lista
    /// confirmado -- grava por cima do arquivo que já tem esse nome.
    OverwriteNamedSession(String),
    /// RF-14.16: exclui de verdade o arquivo da sessão nomeada.
    DeleteNamedSession(PathBuf),
    /// RF-16.4: fechar a janela de configurações (ou encerrar o app) com
    /// alterações pendentes. Quem responde é a própria janela de
    /// configurações, que guarda o rascunho; `App` só vê a resposta.
    SettingsPending,
    /// RF-20.4 (ADR-0075 §6): excluir o perfil `n` da lista da tela de
    /// configurações. Quem responde é a própria janela de configurações, que
    /// tem o rascunho dos perfis; um diálogo de `WindowState` nunca o carrega.
    DeleteProfile(usize),
    /// RF-20.16 (ADR-0075 §3): confiar numa chave de host desconhecida.
    /// Confirmar responde `Confirm` à pergunta `prompt` do painel; qualquer
    /// outra saída (cancelar, `Esc`) responde `Cancel`.
    HostKey(TabId, PaneId, PromptId),
    /// RF-20.12 (ADR-0075 §2): senha ou passphrase. Confirmar responde à
    /// pergunta `prompt` do painel com o texto do campo, **movido**; qualquer
    /// outra saída responde `Cancel`. O campo mascarado não mora aqui (este
    /// enum é `Clone`): vive em `WindowState::secret_input`.
    Secret(TabId, PaneId, PromptId),
    /// Aviso do app (substitui a nota escrita no grid): só informa, não há o
    /// que confirmar -- o diálogo vem sem botão de confirmar.
    Notice,
    /// "Reconectar agora" do aviso de reconexão SSH do painel (ADR-0076 §5).
    ReconnectNow(TabId, PaneId),
    /// "Copiar o trecho" do convite de integração de shell: o trecho vai à
    /// área de transferência.
    ShellInviteCopy(String),
    /// "Não mostrar mais" do convite de integração de shell (RF-3.1,
    /// ADR-0039 §4): a dispensa definitiva.
    ShellInviteDismiss,
    /// "Conectar" do aviso de um painel SSH restaurado que espera o gesto
    /// (RF-20.33): o mesmo que o `Enter` ou o clique no painel.
    ConnectDormant(TabId, PaneId),
}

/// Qual aviso um diálogo é, quando um aviso novo do mesmo tipo o **substitui**
/// em vez de empilhar: o aviso vivo da reconexão de um painel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeSlot {
    Reconnect(TabId, PaneId),
    /// "Conectando a {host}…": vive até o painel conectar ou falhar.
    Connecting(TabId, PaneId),
    /// "Enter para conectar a {nome}": vive até o painel conectar.
    Dormant(TabId, PaneId),
}

/// Onde as teclas vão no diálogo de segredo. Aqui o foco inicial é o **campo**,
/// não o cancelar: o diálogo não é destrutivo, e o que o usuário veio fazer é
/// digitar (ADR-0075 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretFocus {
    Field,
    Remember,
    /// Um dos botões; qual, está em [`ConfirmDialog::focused`].
    Buttons,
}

/// Uma caixa de marcar no corpo de um diálogo de confirmação, com a anatomia da
/// alternância da tela de configurações: "Apagar também a senha e a passphrase
/// salvas" ao excluir um perfil SSH (RF-20.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckboxFacts {
    pub label: String,
    pub checked: bool,
}

/// O que o diálogo de segredo tem além de um diálogo de confirmação: só dado.
/// O **texto digitado** é a única coisa que não mora aqui (este tipo é `Clone`
/// e `Debug`): vive num `MaskedFieldState` em `WindowState::secret_field`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFacts {
    pub remember_label: String,
    /// Sem cofre, a caixa fica desabilitada e leva a nota esmaecida
    /// (RF-20.14).
    pub vault_available: bool,
    pub unavailable_note: String,
    /// O corpo é o caminho de uma chave: trunca no **meio** (ADR-0075 §2).
    pub body_is_path: bool,
    /// A caixa "Lembrar no cofre do sistema", desmarcada ao abrir.
    pub remember: bool,
    pub focus: SecretFocus,
}

/// O tom de uma linha abaixo do corpo (ADR-0075 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTone {
    /// Dado, em mono: cor "Secundário" (`#a8b0bb`, §1.4). Nunca truncado.
    Data,
    /// Aviso, na cor de aviso da §2.14.
    Warning,
    /// Continuação do corpo de um aviso do app: cor do corpo, cortado ao
    /// conteúdo como o corpo.
    Text,
}

/// Uma linha de dado ou de aviso entre o corpo e os botões. O texto de uma
/// linha de dado não passa pelo catálogo (é a impressão digital, como no
/// `ssh`); os rótulos passam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogLine {
    pub text: String,
    pub tone: LineTone,
}

/// O que o diálogo de host desconhecido mostra: dado, não frase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyFacts {
    pub host: String,
    pub algorithm: String,
    pub fingerprint: String,
    /// Tipos de chave que o host já tem registrados (ADR-0074 §3).
    pub known_types: Vec<String>,
    /// Linhas do `known_hosts` que o leitor não entendeu (ADR-0074 §2).
    pub skipped_lines: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialog {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    /// Copiado na abertura junto com os outros três textos: um diálogo
    /// aberto não muda de idioma no meio (ADR-0056 §9).
    pub cancel_label: String,
    /// O botão do meio, só no diálogo de três saídas (RF-16.4); `None` nos
    /// outros, que continuam com dois botões.
    pub discard_label: Option<String>,
    pub action: DialogAction,
    /// O que o botão do meio faz, quando ele existe e não é o do diálogo de
    /// configurações (que responde sozinho): o "Não mostrar mais" do convite.
    pub discard_action: Option<Box<DialogAction>>,
    /// O aviso que um igual substitui (reconexão), em vez de empilhar.
    pub slot: Option<NoticeSlot>,
    /// Linhas entre o corpo e os botões; vazio nos diálogos de sempre.
    pub lines: Vec<DialogLine>,
    /// Só no diálogo de segredo: o campo e a caixa "Lembrar" entram entre o
    /// corpo e os botões.
    /// Em caixa: o diálogo viaja em enums pequenos (`TabCloseOutcome`), e só um
    /// em cada centena é de segredo.
    pub secret: Option<Box<SecretFacts>>,
    /// Uma caixa de marcar entre o corpo e os botões, sem campo (o diálogo de
    /// excluir perfil). Exclusiva do `secret`, que tem a própria.
    pub checkbox: Option<Box<CheckboxFacts>>,
    /// O corpo é a linha de erro (segredo recusado): pintado na cor de erro.
    pub body_is_error: bool,
    focused: DialogButton,
    /// O botão sob o cursor, só no diálogo de três saídas: os de dois botões
    /// não têm realce de hover.
    hovered: Option<DialogButton>,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
        action: DialogAction,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
            discard_label: None,
            action,
            discard_action: None,
            slot: None,
            lines: Vec::new(),
            secret: None,
            checkbox: None,
            body_is_error: false,
            focused: DialogButton::Cancel,
            hovered: None,
        }
    }

    pub const fn focused(&self) -> DialogButton {
        self.focused
    }

    /// RF-1.6 (ADR-0017): fechar aba com processo ativo. `title` é o título
    /// da aba, que vem de um programa e entra na frase como chegou.
    pub fn close_tab(catalog: &Catalog, title: &str, tab: TabId) -> Self {
        Self::new(
            msg::dialog::close_tab::title(catalog),
            msg::dialog::close_tab::body(catalog, title),
            msg::dialog::close_tab::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::CloseTab(tab),
        )
    }

    /// RF-6.10: fechar um painel (não o último da aba) com processo ativo.
    pub fn close_pane(catalog: &Catalog, title: &str, tab: TabId, pane: PaneId) -> Self {
        Self::new(
            msg::dialog::close_pane::title(catalog),
            msg::dialog::close_pane::body(catalog, title),
            msg::dialog::close_pane::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::ClosePane(tab, pane),
        )
    }

    /// RF-10.23 (ADR-0015): fechar janela. O corpo escolhe pela contagem de
    /// **abas** de verdade, não de painéis: uma aba dividida em três painéis
    /// não é "mais de uma aba".
    pub fn close_window(catalog: &Catalog, real_tab_count: usize) -> Self {
        let body = if real_tab_count > 1 {
            msg::dialog::close_window::body_tabs(catalog)
        } else {
            msg::dialog::close_window::body_program(catalog)
        };
        Self::new(
            msg::dialog::close_window::title(catalog),
            body,
            msg::dialog::close_window::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::CloseWindow,
        )
    }

    /// RF-2.23 (`group.close_all`): a forma de plural vem do idioma.
    pub fn close_group(catalog: &Catalog, tab_count: usize, group: GroupId) -> Self {
        Self::new(
            msg::dialog::close_group::title(catalog),
            msg::dialog::close_group::body(catalog, tab_count),
            msg::dialog::close_group::confirm(catalog, tab_count),
            msg::dialog::cancel(catalog),
            DialogAction::CloseGroup(group),
        )
    }

    /// RF-14.16: excluir uma sessão salva. `name` é o que o usuário deu à
    /// sessão.
    pub fn delete_session(catalog: &Catalog, name: &str, file: PathBuf) -> Self {
        Self::new(
            msg::dialog::delete_session::title(catalog),
            msg::dialog::delete_session::body(catalog, name),
            msg::dialog::delete_session::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::DeleteNamedSession(file),
        )
    }

    /// RF-14.5: o nome digitado já existe na lista. `existing` é o nome da
    /// entrada que seria substituída, `typed` o que foi digitado (o que
    /// `OverwriteNamedSession` grava).
    pub fn overwrite_session(catalog: &Catalog, existing: &str, typed: String) -> Self {
        Self::new(
            msg::dialog::overwrite_session::title(catalog, existing),
            msg::dialog::overwrite_session::body(catalog),
            msg::dialog::overwrite_session::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::OverwriteNamedSession(typed),
        )
    }

    /// RF-16.4 (ADR-0060 §4): fechar a janela de configurações com
    /// `changes` alterações pendentes. Três saídas, foco inicial no cancelar.
    pub fn settings_pending(catalog: &Catalog, changes: usize) -> Self {
        let mut dialog = Self::new(
            msg::settings::dialog::close_title(catalog),
            msg::settings::dialog::close_body(catalog, changes),
            msg::settings::dialog::save_and_close(catalog),
            msg::settings::dialog::cancel(catalog),
            DialogAction::SettingsPending,
        );
        dialog.discard_label = Some(msg::settings::dialog::discard_and_close(catalog));
        dialog
    }

    /// RF-20.4 (ADR-0075 §6): excluir o perfil `index` da lista. Com
    /// `offers_forget` (perfil SSH que nenhum outro compartilha), a caixa
    /// "Apagar também a senha e a passphrase salvas" vem **marcada**. Excluir é
    /// o confirmar destrutivo de sempre, e o foco inicial é o cancelar.
    pub fn delete_profile(
        catalog: &Catalog,
        name: &str,
        index: usize,
        offers_forget: bool,
    ) -> Self {
        let mut dialog = Self::new(
            msg::settings::dialog::delete_profile_title(catalog, name),
            msg::settings::dialog::delete_profile_body(catalog),
            msg::settings::dialog::delete_profile_confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::DeleteProfile(index),
        );
        if offers_forget {
            dialog.checkbox = Some(Box::new(CheckboxFacts {
                label: msg::settings::dialog::delete_profile_forget(catalog),
                checked: true,
            }));
        }
        dialog
    }

    /// Aviso do app (substitui a nota que ia ao grid): `text` quebra em linhas
    /// -- a primeira é o corpo, as demais vêm abaixo. Só o botão de fechar.
    pub fn notice(catalog: &Catalog, title: impl Into<String>, text: &str) -> Self {
        let mut dialog = Self::new(
            title,
            "",
            "",
            msg::dialog::notice::close(catalog),
            DialogAction::Notice,
        );
        dialog.set_text(text);
        dialog
    }

    /// Aviso de erro: o corpo na cor de erro.
    pub fn error_notice(catalog: &Catalog, title: impl Into<String>, text: &str) -> Self {
        let mut dialog = Self::notice(catalog, title, text);
        dialog.body_is_error = true;
        dialog
    }

    /// Aviso vivo da reconexão SSH de um painel (ADR-0076): `text` diz o que
    /// derrubou a conexão (quando se sabe) e o que o ciclo espera agora, em
    /// linhas. "Reconectar agora" antecipa a próxima tentativa.
    pub fn reconnect(catalog: &Catalog, tab: TabId, pane: PaneId, text: &str) -> Self {
        let mut dialog = Self::new(
            msg::dialog::notice::title_ssh(catalog),
            "",
            msg::dialog::notice::reconnect_now(catalog),
            msg::dialog::notice::close(catalog),
            DialogAction::ReconnectNow(tab, pane),
        );
        dialog.set_text(text);
        dialog.slot = Some(NoticeSlot::Reconnect(tab, pane));
        dialog
    }

    /// Aviso vivo de uma conexão SSH em andamento: só informa e fecha sozinho
    /// quando o painel conecta ou falha (`NoticeSlot::Connecting`).
    pub fn ssh_connecting(catalog: &Catalog, tab: TabId, pane: PaneId, text: &str) -> Self {
        let mut dialog = Self::notice(catalog, msg::dialog::notice::title_ssh(catalog), text);
        dialog.slot = Some(NoticeSlot::Connecting(tab, pane));
        dialog
    }

    /// Aviso de um painel SSH restaurado que não conectou sozinho (RF-20.33):
    /// **Conectar** com o foco inicial -- o diálogo não é destrutivo e o que o
    /// usuário veio fazer é conectar -- e **Fechar**.
    pub fn ssh_dormant(catalog: &Catalog, tab: TabId, pane: PaneId, text: &str) -> Self {
        let mut dialog = Self::new(
            msg::dialog::notice::title_ssh(catalog),
            "",
            msg::dialog::notice::connect(catalog),
            msg::dialog::notice::close(catalog),
            DialogAction::ConnectDormant(tab, pane),
        );
        dialog.set_text(text);
        dialog.slot = Some(NoticeSlot::Dormant(tab, pane));
        dialog.focused = DialogButton::Confirm;
        dialog
    }

    /// Convite de integração de shell (RF-3.1, ADR-0039): `snippet`, quando há
    /// um pronto para o shell, ganha o botão de copiar.
    pub fn shell_invite(catalog: &Catalog, text: &str, snippet: Option<&str>) -> Self {
        let mut dialog = Self::new(
            msg::dialog::notice::title_shell_invite(catalog),
            "",
            if snippet.is_some() {
                msg::dialog::notice::copy_snippet(catalog)
            } else {
                String::new()
            },
            msg::dialog::notice::close(catalog),
            match snippet {
                Some(snippet) => DialogAction::ShellInviteCopy(snippet.to_owned()),
                None => DialogAction::Notice,
            },
        );
        dialog.discard_label = Some(msg::dialog::notice::dont_show_again(catalog));
        dialog.discard_action = Some(Box::new(DialogAction::ShellInviteDismiss));
        dialog.set_text(text);
        dialog
    }

    /// Põe o texto: primeira linha no corpo, o resto em `lines`. O fim de
    /// linha pode vir como `\r\n` ou `\n`.
    fn set_text(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n");
        let mut rows = normalized.trim_end_matches('\n').split('\n');
        self.body = rows.next().unwrap_or_default().to_owned();
        self.lines = rows
            .map(|row| DialogLine {
                text: row.to_owned(),
                tone: LineTone::Text,
            })
            .collect();
    }

    /// Há botão de confirmar (o aviso que só informa não tem).
    pub fn has_confirm(&self) -> bool {
        !self.confirm_label.is_empty()
    }

    /// A ação do botão do meio, se o diálogo tem uma.
    pub fn discard_action(&self) -> Option<DialogAction> {
        self.discard_action.as_deref().cloned()
    }

    /// Troca o conteúdo de um aviso vivo por `other`, que o substitui,
    /// mantendo o botão focado.
    pub fn replace_with(&mut self, other: ConfirmDialog) {
        let focused = self.focused;
        *self = other;
        if self.buttons().contains(&focused) {
            self.focused = focused;
        }
    }

    /// A caixa de marcar está marcada (e existe).
    pub fn checkbox_checked(&self) -> bool {
        self.checkbox
            .as_ref()
            .is_some_and(|checkbox| checkbox.checked)
    }

    pub fn toggle_checkbox(&mut self) {
        if let Some(checkbox) = self.checkbox.as_mut() {
            checkbox.checked = !checkbox.checked;
        }
    }

    /// RF-20.16 (ADR-0075 §3): host desconhecido. O corpo tem uma linha; abaixo
    /// vêm o tipo da chave e a impressão digital (dado, sem truncar) e, quando
    /// há, os avisos de outro tipo registrado e de linhas puladas. O foco
    /// inicial é o cancelar, como em todo diálogo: um `Enter` distraído não
    /// confia num host.
    pub fn host_key(catalog: &Catalog, facts: &HostKeyFacts, action: DialogAction) -> Self {
        let mut dialog = Self::new(
            msg::dialog::host_key::title(catalog, &facts.host),
            msg::dialog::host_key::body(catalog),
            msg::dialog::host_key::confirm(catalog),
            msg::dialog::cancel(catalog),
            action,
        );
        let data = |text: String| DialogLine {
            text,
            tone: LineTone::Data,
        };
        dialog.lines = vec![
            data(msg::dialog::host_key::type_line(catalog, &facts.algorithm)),
            data(msg::dialog::host_key::fingerprint_label(catalog)),
            data(facts.fingerprint.clone()),
        ];
        if !facts.known_types.is_empty() {
            dialog.lines.push(DialogLine {
                text: msg::dialog::host_key::other_type(catalog, facts.known_types.join(", ")),
                tone: LineTone::Warning,
            });
        }
        if facts.skipped_lines > 0 {
            dialog.lines.push(DialogLine {
                text: msg::dialog::host_key::skipped_lines(catalog, facts.skipped_lines),
                tone: LineTone::Warning,
            });
        }
        dialog
    }

    /// RF-20.12 (ADR-0075 §2): senha ou passphrase. Todo texto é do app: o que
    /// o servidor escreveu num keyboard-interactive não chega aqui. Segredo
    /// recusado (`retry`) põe a linha de erro no lugar do corpo.
    pub fn secret(
        catalog: &Catalog,
        label: &SecretLabel,
        vault_available: bool,
        retry: bool,
        action: DialogAction,
    ) -> Self {
        let (title, plain_body, body_is_path, refused) = match label {
            SecretLabel::Password { user, host } => {
                let target = format!("{user}@{host}");
                (
                    msg::dialog::secret::title_password(catalog, &target),
                    target,
                    false,
                    msg::dialog::secret::refused_password(catalog),
                )
            }
            SecretLabel::KeyPassphrase { path } => (
                msg::dialog::secret::title_passphrase(catalog),
                path.display().to_string(),
                true,
                msg::dialog::secret::refused_passphrase(catalog),
            ),
        };
        let mut dialog = Self::new(
            title,
            if retry { refused } else { plain_body },
            msg::dialog::secret::confirm(catalog),
            msg::dialog::cancel(catalog),
            action,
        );
        dialog.body_is_error = retry;
        dialog.secret = Some(Box::new(SecretFacts {
            remember_label: msg::dialog::secret::remember(catalog),
            vault_available,
            unavailable_note: msg::dialog::secret::remember_unavailable(catalog),
            body_is_path: body_is_path && !retry,
            remember: false,
            focus: SecretFocus::Field,
        }));
        dialog
    }

    /// A caixa só vale com cofre (RF-20.14).
    pub fn toggle_remember(&mut self) {
        if let Some(facts) = self.secret.as_mut()
            && facts.vault_available
        {
            facts.remember = !facts.remember;
        }
    }

    /// Foco no campo ou na caixa, por clique.
    pub fn focus_secret(&mut self, focus: SecretFocus) {
        if let Some(facts) = self.secret.as_mut() {
            facts.focus = focus;
        }
    }

    /// Anda `delta` posições, em círculo, na ordem de leitura e de `Tab`:
    /// campo, caixa (se há cofre), Cancelar, Conectar.
    pub fn step_secret_focus(&mut self, delta: i32) {
        let Some(facts) = self.secret.as_ref() else {
            return;
        };
        let mut order = vec![(SecretFocus::Field, DialogButton::Cancel)];
        if facts.vault_available {
            order.push((SecretFocus::Remember, DialogButton::Cancel));
        }
        order.push((SecretFocus::Buttons, DialogButton::Cancel));
        order.push((SecretFocus::Buttons, DialogButton::Confirm));
        let at = order
            .iter()
            .position(|(focus, button)| {
                *focus == facts.focus && (*focus != SecretFocus::Buttons || *button == self.focused)
            })
            .unwrap_or(0) as i32;
        let (focus, button) = order[(at + delta).rem_euclid(order.len() as i32) as usize];
        if focus == SecretFocus::Buttons {
            self.focused = button;
        }
        if let Some(facts) = self.secret.as_mut() {
            facts.focus = focus;
        }
    }

    /// O botão sob o cursor, realçado só no diálogo de três saídas.
    pub const fn hovered(&self) -> Option<DialogButton> {
        self.hovered
    }

    pub fn set_hovered(&mut self, button: Option<DialogButton>) {
        self.hovered = button;
    }

    /// Os botões do diálogo, da esquerda para a direita: a ordem de leitura e
    /// de `Tab`.
    pub fn buttons(&self) -> Vec<DialogButton> {
        let mut buttons = vec![DialogButton::Cancel];
        if self.discard_label.is_some() {
            buttons.push(DialogButton::Discard);
        }
        if self.has_confirm() {
            buttons.push(DialogButton::Confirm);
        }
        buttons
    }

    /// Navegação por teclado entre os botões -- a espec. não descreve a
    /// tecla, mas um diálogo só-mouse não seria alcançável do teclado além
    /// do default seguro (`Enter` = cancelar). `Tab`, `Left` e `Right`
    /// andam em círculo: com dois botões é "trocar".
    pub fn toggle_focus(&mut self) {
        self.step_focus(1);
    }

    /// Anda `delta` botões, em círculo (`Shift+Tab` e `Left` usam -1).
    pub fn step_focus(&mut self, delta: i32) {
        let buttons = self.buttons();
        let at = buttons
            .iter()
            .position(|button| *button == self.focused)
            .unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(buttons.len() as i32) as usize;
        self.focused = buttons[next];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

    /// Regressão achada na verificação ao vivo da etapa 5 dos painéis: uma
    /// janela com uma aba só, dividida em painéis (`real_tab_count == 1`),
    /// não pode dizer "mais de uma aba aberta" -- `request_close_window`
    /// decidia essa frase pela contagem de **painéis**, não de abas.
    #[test]
    fn close_window_body_names_the_program_for_a_single_tab_with_many_panes() {
        let pt = test_support::pt_br();
        assert_eq!(
            ConfirmDialog::close_window(&pt, 1).body,
            "Esta janela tem um programa em primeiro plano."
        );
        let en = test_support::en_us();
        assert_eq!(
            ConfirmDialog::close_window(&en, 1).body,
            "This window has a program running in the foreground."
        );
    }

    #[test]
    fn close_window_body_names_more_than_one_tab_when_there_really_is() {
        let pt = test_support::pt_br();
        assert_eq!(
            ConfirmDialog::close_window(&pt, 2).body,
            "Esta janela tem mais de uma aba aberta."
        );
        let en = test_support::en_us();
        assert_eq!(
            ConfirmDialog::close_window(&en, 2).body,
            "This window has more than one tab open."
        );
    }

    #[test]
    fn dialogs_copy_all_four_texts_when_they_open() {
        let pt = test_support::pt_br();
        let dialog = ConfirmDialog::close_tab(&pt, "vim", TabId::new(1));
        assert_eq!(dialog.title, "Fechar aba?");
        assert_eq!(
            dialog.body,
            "\"vim\" tem um programa em primeiro plano. Fechar mesmo assim?"
        );
        assert_eq!(dialog.confirm_label, "Fechar aba");
        assert_eq!(dialog.cancel_label, "Cancelar");
        assert_eq!(dialog.action, DialogAction::CloseTab(TabId::new(1)));
    }

    #[test]
    fn close_group_dialog_pluralizes_body_and_button_in_both_languages() {
        let pt = test_support::pt_br();
        let one = ConfirmDialog::close_group(&pt, 1, GroupId::new(1));
        assert_eq!(one.body, "Isso fecha 1 aba.");
        assert_eq!(one.confirm_label, "Fechar grupo (1 aba)");
        let many = ConfirmDialog::close_group(&pt, 3, GroupId::new(1));
        assert_eq!(many.body, "Isso fecha 3 abas.");
        assert_eq!(many.confirm_label, "Fechar grupo (3 abas)");

        let en = test_support::en_us();
        let one = ConfirmDialog::close_group(&en, 1, GroupId::new(1));
        assert_eq!(one.body, "This closes 1 tab.");
        assert_eq!(one.confirm_label, "Close group (1 tab)");
        let many = ConfirmDialog::close_group(&en, 3, GroupId::new(1));
        assert_eq!(many.body, "This closes 3 tabs.");
        assert_eq!(many.confirm_label, "Close group (3 tabs)");
    }

    #[test]
    fn session_dialogs_carry_the_user_typed_name_as_it_is() {
        let pt = test_support::pt_br();
        let overwrite = ConfirmDialog::overwrite_session(&pt, "api", "api ".to_owned());
        assert_eq!(overwrite.title, "Sobrescrever a sessão «api»?");
        assert_eq!(
            overwrite.action,
            DialogAction::OverwriteNamedSession("api ".to_owned())
        );
        let delete = ConfirmDialog::delete_session(&pt, "api", PathBuf::from("api.json"));
        assert_eq!(delete.body, "Isso remove «api» permanentemente.");
        assert_eq!(delete.confirm_label, "Excluir");
    }

    #[test]
    fn a_notice_splits_into_body_and_lines_and_has_only_a_close_button() {
        let pt = test_support::pt_br();
        let dialog = ConfirmDialog::notice(&pt, "t", "um\r\ndois\r\ntrês\r\n");
        assert_eq!(dialog.body, "um");
        let rest: Vec<&str> = dialog.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(rest, ["dois", "três"]);
        assert!(!dialog.has_confirm());
        assert_eq!(dialog.buttons(), vec![DialogButton::Cancel]);
        assert_eq!(dialog.cancel_label, "Fechar");
    }

    #[test]
    fn the_shell_invite_offers_copy_only_with_a_snippet() {
        let pt = test_support::pt_br();
        let with = ConfirmDialog::shell_invite(&pt, "texto", Some("echo"));
        assert_eq!(
            with.action,
            DialogAction::ShellInviteCopy("echo".to_owned())
        );
        assert_eq!(
            with.discard_action(),
            Some(DialogAction::ShellInviteDismiss)
        );
        assert_eq!(
            with.buttons(),
            vec![
                DialogButton::Cancel,
                DialogButton::Discard,
                DialogButton::Confirm
            ]
        );
        let without = ConfirmDialog::shell_invite(&pt, "texto", None);
        assert!(!without.has_confirm());
        assert_eq!(
            without.buttons(),
            vec![DialogButton::Cancel, DialogButton::Discard]
        );
    }

    #[test]
    fn a_live_notice_keeps_its_focused_button_when_replaced() {
        let pt = test_support::pt_br();
        let (tab, pane) = (TabId::new(1), PaneId::new(1));
        let mut open = ConfirmDialog::reconnect(&pt, tab, pane, "a");
        open.toggle_focus();
        assert_eq!(open.focused(), DialogButton::Confirm);
        open.replace_with(ConfirmDialog::reconnect(&pt, tab, pane, "b"));
        assert_eq!(open.body, "b");
        assert_eq!(open.focused(), DialogButton::Confirm);
        assert_eq!(open.slot, Some(NoticeSlot::Reconnect(tab, pane)));
    }

    #[test]
    fn the_connecting_notice_only_informs_and_lives_in_its_slot() {
        let en = test_support::en_us();
        let (tab, pane) = (TabId::new(1), PaneId::new(2));
        let dialog = ConfirmDialog::ssh_connecting(&en, tab, pane, "connecting to h…");
        assert_eq!(dialog.body, "connecting to h…");
        assert!(!dialog.has_confirm());
        assert_eq!(dialog.slot, Some(NoticeSlot::Connecting(tab, pane)));
    }

    #[test]
    fn the_dormant_notice_starts_focused_on_connect_so_enter_connects() {
        let en = test_support::en_us();
        let (tab, pane) = (TabId::new(1), PaneId::new(2));
        let dialog = ConfirmDialog::ssh_dormant(&en, tab, pane, "Enter to connect to lab");
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        assert_eq!(dialog.confirm_label, "Connect");
        assert_eq!(dialog.cancel_label, "Close");
        assert_eq!(dialog.action, DialogAction::ConnectDormant(tab, pane));
        assert_eq!(dialog.slot, Some(NoticeSlot::Dormant(tab, pane)));
    }

    #[test]
    fn starts_focused_on_cancel() {
        let dialog = ConfirmDialog::new("t", "b", "Fechar", "Cancelar", DialogAction::CloseWindow);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }

    #[test]
    fn toggle_focus_swaps_between_the_two_buttons() {
        let mut dialog =
            ConfirmDialog::new("t", "b", "Fechar", "Cancelar", DialogAction::CloseWindow);
        dialog.toggle_focus();
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.toggle_focus();
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }

    #[test]
    fn the_delete_profile_dialog_checks_the_forget_box_only_when_offered() {
        let pt = test_support::pt_br();
        let mut dialog = ConfirmDialog::delete_profile(&pt, "prod", 1, true);
        assert_eq!(dialog.action, DialogAction::DeleteProfile(1));
        assert_eq!(dialog.focused(), DialogButton::Cancel);
        assert!(dialog.checkbox_checked());
        dialog.toggle_checkbox();
        assert!(!dialog.checkbox_checked());
        dialog.toggle_checkbox();
        assert!(dialog.checkbox_checked());

        let plain = ConfirmDialog::delete_profile(&pt, "prod", 1, false);
        assert!(plain.checkbox.is_none());
        assert!(!plain.checkbox_checked());
    }

    #[test]
    fn the_settings_dialog_has_three_buttons_and_starts_on_cancel() {
        let dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 3);
        assert_eq!(dialog.title, "Fechar com alterações pendentes?");
        assert_eq!(dialog.body, "Há 3 alterações não salvas.");
        assert_eq!(dialog.confirm_label, "Salvar e fechar");
        assert_eq!(dialog.discard_label.as_deref(), Some("Descartar e fechar"));
        assert_eq!(dialog.cancel_label, "Cancelar");
        assert_eq!(dialog.focused(), DialogButton::Cancel);
        assert_eq!(
            dialog.buttons(),
            [
                DialogButton::Cancel,
                DialogButton::Discard,
                DialogButton::Confirm
            ]
        );
    }

    #[test]
    fn focus_goes_around_all_three_buttons_both_ways() {
        let mut dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 1);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Discard);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
        dialog.step_focus(-1);
        assert_eq!(dialog.focused(), DialogButton::Confirm);
    }

    fn facts(known_types: &[&str], skipped_lines: usize) -> HostKeyFacts {
        HostKeyFacts {
            host: "10.4.1.20".to_owned(),
            algorithm: "ssh-ed25519".to_owned(),
            fingerprint: "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE".to_owned(),
            known_types: known_types.iter().map(|t| (*t).to_owned()).collect(),
            skipped_lines,
        }
    }

    fn host_key_action() -> DialogAction {
        DialogAction::HostKey(TabId::new(2), PaneId::new(1), PromptId::new(7))
    }

    /// ADR-0014 / ADR-0075 §3: o foco inicial é o cancelar -- um `Enter`
    /// distraído não confia num host.
    #[test]
    fn the_host_key_dialog_starts_on_cancel_and_enter_does_not_trust() {
        let dialog =
            ConfirmDialog::host_key(&test_support::pt_br(), &facts(&[], 0), host_key_action());

        assert_eq!(dialog.focused(), DialogButton::Cancel);
        assert_eq!(
            dialog.buttons(),
            [DialogButton::Cancel, DialogButton::Confirm]
        );
        assert_eq!(dialog.action, host_key_action());
    }

    #[test]
    fn the_host_key_dialog_reads_as_title_body_data_and_two_buttons() {
        let pt = test_support::pt_br();
        let dialog = ConfirmDialog::host_key(&pt, &facts(&[], 0), host_key_action());

        assert_eq!(dialog.title, "Host desconhecido: 10.4.1.20");
        assert_eq!(
            dialog.body,
            "A autenticidade deste host não pode ser confirmada."
        );
        assert_eq!(dialog.cancel_label, "Cancelar");
        assert_eq!(dialog.confirm_label, "Confiar e conectar");
        assert_eq!(dialog.discard_label, None);
        let texts: Vec<&str> = dialog.lines.iter().map(|l| l.text.as_str()).collect();
        // The labels come from the catalog; the type and the fingerprint do not.
        assert_eq!(
            texts,
            [
                "Tipo: ssh-ed25519",
                "Impressão digital:",
                "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE",
            ]
        );
        assert!(dialog.lines.iter().all(|l| l.tone == LineTone::Data));
    }

    #[test]
    fn the_host_key_dialog_adds_a_warning_for_another_registered_type_and_for_skipped_lines() {
        let pt = test_support::pt_br();

        let other_type = ConfirmDialog::host_key(
            &pt,
            &facts(&["ecdsa-sha2-nistp256", "ssh-rsa"], 0),
            host_key_action(),
        );
        let warnings: Vec<&str> = other_type
            .lines
            .iter()
            .filter(|l| l.tone == LineTone::Warning)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(
            warnings,
            ["Outro tipo já registrado: ecdsa-sha2-nistp256, ssh-rsa"]
        );

        let skipped = ConfirmDialog::host_key(&pt, &facts(&[], 1), host_key_action());
        assert_eq!(
            skipped.lines.last().map(|l| l.text.as_str()),
            Some("1 linha do known_hosts não pôde ser lida.")
        );
        let many = ConfirmDialog::host_key(&pt, &facts(&[], 3), host_key_action());
        assert_eq!(
            many.lines.last().map(|l| l.text.as_str()),
            Some("3 linhas do known_hosts não puderam ser lidas.")
        );

        // Neither: just the three data lines.
        assert_eq!(
            ConfirmDialog::host_key(&pt, &facts(&[], 0), host_key_action())
                .lines
                .len(),
            3
        );
    }

    #[test]
    fn the_host_key_dialog_is_translated_but_its_data_is_not() {
        for locale in ["en_US", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            let dialog =
                ConfirmDialog::host_key(&catalog, &facts(&["ssh-rsa"], 2), host_key_action());
            assert!(dialog.title.contains("10.4.1.20"), "{locale}");
            // Fingerprint and algorithm travel verbatim.
            assert!(
                dialog.lines.iter().any(|l| l.text.contains("ssh-ed25519")),
                "{locale}"
            );
            assert!(
                dialog
                    .lines
                    .iter()
                    .any(|l| l.text == "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE"),
                "{locale}"
            );
            assert_ne!(dialog.confirm_label, "Confiar e conectar", "{locale}");
        }
    }

    #[test]
    fn the_ordinary_dialogs_have_no_extra_lines() {
        let pt = test_support::pt_br();
        assert!(ConfirmDialog::close_window(&pt, 2).lines.is_empty());
        assert!(ConfirmDialog::settings_pending(&pt, 1).lines.is_empty());
    }

    #[test]
    fn a_two_button_dialog_never_reaches_the_discard_button() {
        let mut dialog =
            ConfirmDialog::new("t", "b", "Fechar", "Cancelar", DialogAction::CloseWindow);
        assert_eq!(
            dialog.buttons(),
            [DialogButton::Cancel, DialogButton::Confirm]
        );
        dialog.step_focus(-1);
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }
}
