// SPDX-License-Identifier: GPL-3.0-or-later

//! O rascunho do grupo "Perfis" (ADR-0075 §6, PRD-020 RF-20.3 e RF-20.4): os
//! perfis de `[[profiles.list]]` como a tela os edita, com as alterações
//! pendentes por perfil.
//!
//! É o estado **paralelo** do `Draft` de opções, no molde de `Shortcuts`: o
//! catálogo de opções tem chaves `&'static str`, e a lista de perfis é dado do
//! usuário. Estado puro, sem `winit` nem disco.
//!
//! - cada perfil de trabalho sabe **de qual perfil do arquivo veio** (`origin`,
//!   o `id` de lá): é por ele que a edição acha o elemento, nunca pela posição;
//! - o **tipo** (local ou SSH) é escolhido na criação e não muda depois: não há
//!   função que o troque;
//! - a validação **é a do parser** (`porecatu-config`, ADR-0070 §1 e ADR-0072
//!   §6): o perfil é escrito num texto de prova e lido de volta, então a tela
//!   nunca discorda do que o app aceita. Um perfil inválido fica pendente, **não
//!   entra** nas edições e não impede salvar os outros;
//! - o esquecimento de senha e passphrase no cofre só acontece **no Salvar**
//!   (`commit`), nunca ao excluir.

use std::collections::BTreeMap;
use std::path::PathBuf;

use porecatu_config::{
    ConfigDocument, Edit, EditValue, KeyPath, ProfileDef, ProfileKind, ProfileProblemKind, SshAuth,
    SshImportCandidate, SshProfile,
};
use porecatu_term::Destination;

use super::profile_editor::{EditorProblems, Focus, ProfileEditor, Target};

/// O badge que o parser dá a um perfil SSH sem `badge` (ADR-0072 §6).
pub(crate) const DEFAULT_SSH_BADGE: &str = "SSH";

/// Onde o array de perfis mora no arquivo.
pub(crate) const LIST: &str = "profiles.list";

/// Os conjuntos de métodos que o seletor de autenticação oferece, na ordem da
/// tela. O primeiro é a ordem default (RF-20.9). Uma lista do arquivo que não é
/// nenhum destes fica como está e aparece como "personalizada".
pub(crate) const AUTH_PRESETS: [&[SshAuth]; 5] = [
    &[SshAuth::Agent, SshAuth::Key, SshAuth::Password],
    &[SshAuth::Agent],
    &[SshAuth::Key],
    &[SshAuth::Password],
    &[SshAuth::Key, SshAuth::Password],
];

/// O campo do formulário de um perfil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Field {
    Id,
    Name,
    Badge,
    Program,
    Cwd,
    Host,
    Port,
    User,
    Auth,
    IdentityFile,
}

/// Os campos de texto que cada tipo mostra, na ordem do formulário
/// (ADR-0075 §6: perfil local não mostra host; perfil SSH não mostra programa).
pub(crate) const LOCAL_FIELDS: [Field; 5] = [
    Field::Name,
    Field::Id,
    Field::Badge,
    Field::Program,
    Field::Cwd,
];
pub(crate) const SSH_FIELDS: [Field; 7] = [
    Field::Name,
    Field::Id,
    Field::Badge,
    Field::Host,
    Field::Port,
    Field::User,
    Field::IdentityFile,
];

/// As duas listas do formulário de um perfil local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListKind {
    Args,
    Env,
}

/// O perfil como o formulário o tem: texto cru, que pode estar inválido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Form {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) badge: String,
    pub(crate) kind: ProfileKind,
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    /// Pares na ordem das linhas; o arquivo guarda um mapa.
    pub(crate) env: Vec<(String, String)>,
    pub(crate) cwd: String,
    pub(crate) host: String,
    /// Texto: pode não ser número enquanto se digita.
    pub(crate) port: String,
    pub(crate) user: String,
    pub(crate) auth: Vec<SshAuth>,
    pub(crate) identity_file: String,
}

impl Form {
    fn from_def(def: &ProfileDef) -> Self {
        let mut form = Self {
            id: def.id.clone(),
            name: def.name.clone(),
            badge: def.badge.clone(),
            kind: def.kind,
            program: def.program.clone(),
            args: def.args.clone(),
            env: def
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            cwd: def.cwd.clone(),
            host: String::new(),
            port: String::new(),
            user: String::new(),
            auth: Vec::new(),
            identity_file: String::new(),
        };
        if let Some(ssh) = &def.ssh {
            // O parser dá `SSH` ao perfil sem badge; o formulário o mostra
            // vazio, como na criação, senão o que acabou de ser gravado nunca
            // igualaria o que a tela tem.
            if form.badge == DEFAULT_SSH_BADGE {
                form.badge.clear();
            }
            form.host = ssh.host.clone();
            form.port = ssh.port.to_string();
            form.user = ssh.user.clone();
            form.auth = ssh.auth.clone();
            form.identity_file = ssh.identity_file.clone();
        }
        form
    }

    fn new_local(id: String, name: &str, badge: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            badge: badge.to_owned(),
            kind: ProfileKind::Local,
            program: String::new(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: String::new(),
            host: String::new(),
            port: String::new(),
            user: String::new(),
            auth: Vec::new(),
            identity_file: String::new(),
        }
    }

    fn new_ssh(id: String, name: &str) -> Self {
        Self {
            kind: ProfileKind::Ssh,
            // Vazio: o parser dá o `SSH` de sempre (RF-20.20).
            badge: String::new(),
            port: "22".to_owned(),
            auth: AUTH_PRESETS[0].to_vec(),
            ..Self::new_local(id, name, "")
        }
    }

    /// Os campos de texto que este tipo mostra.
    pub(crate) fn fields(&self) -> &'static [Field] {
        match self.kind {
            ProfileKind::Local => &LOCAL_FIELDS,
            ProfileKind::Ssh => &SSH_FIELDS,
        }
    }

    pub(crate) fn text(&self, field: Field) -> &str {
        match field {
            Field::Id => &self.id,
            Field::Name => &self.name,
            Field::Badge => &self.badge,
            Field::Program => &self.program,
            Field::Cwd => &self.cwd,
            Field::Host => &self.host,
            Field::Port => &self.port,
            Field::User => &self.user,
            Field::IdentityFile => &self.identity_file,
            Field::Auth => "",
        }
    }

    /// Qual dos [`AUTH_PRESETS`] é a lista, ou `None` se é personalizada.
    pub(crate) fn auth_preset(&self) -> Option<usize> {
        // Sem `auth` e sem arquivo de chave, a ordem de sempre pula a chave
        // (RF-20.9): é o default, o primeiro conjunto, e não uma ordem do usuário.
        if self.auth == [SshAuth::Agent, SshAuth::Password] && self.identity_file.is_empty() {
            return Some(0);
        }
        AUTH_PRESETS.iter().position(|preset| *preset == self.auth)
    }

    fn auth_names(&self) -> Vec<String> {
        self.auth
            .iter()
            .map(|method| {
                match method {
                    SshAuth::Agent => "agent",
                    SshAuth::Key => "key",
                    SshAuth::Password => "password",
                }
                .to_owned()
            })
            .collect()
    }

    /// O valor que `key` tem no arquivo, ou `None` se a chave não é escrita.
    fn value_of(&self, key: &str) -> Option<EditValue> {
        let text = |value: &str| Some(EditValue::String(value.to_owned()));
        let optional = |value: &str| (!value.is_empty()).then(|| EditValue::String(value.into()));
        let local = self.kind == ProfileKind::Local;
        match key {
            "id" => text(&self.id),
            "name" => text(&self.name),
            "type" => (!local).then(|| EditValue::String("ssh".to_owned())),
            // Local: obrigatório (o parser o cobra). SSH: vazio é o default.
            "badge" if local => text(&self.badge),
            // `SSH` é o que o parser dá sem `badge`: escrevê-lo é o mesmo que
            // omitir, e o que volta do arquivo nunca o distingue.
            "badge" => optional(&self.badge).filter(|_| self.badge != DEFAULT_SSH_BADGE),
            "program" => local.then(|| EditValue::String(self.program.clone())),
            // Um item vazio é o espaço de quem acabou de apertar "Adicionar":
            // não é escrito, e não conta como alteração.
            "args" => {
                let args: Vec<String> = self
                    .args
                    .iter()
                    .filter(|a| !a.is_empty())
                    .cloned()
                    .collect();
                (local && !args.is_empty()).then_some(EditValue::StringList(args))
            }
            "env" => {
                let env: BTreeMap<String, String> = self
                    .env
                    .iter()
                    .filter(|(name, value)| !(name.is_empty() && value.is_empty()))
                    .cloned()
                    .collect();
                (local && !env.is_empty()).then_some(EditValue::StringMap(env))
            }
            "cwd" => local.then(|| optional(&self.cwd)).flatten(),
            "host" => (!local).then(|| EditValue::String(self.host.clone())),
            // Não numérico vira 0, que o parser recusa como `PortInvalid`.
            "port" => {
                (!local).then(|| EditValue::Integer(self.port.trim().parse::<i64>().unwrap_or(0)))
            }
            "user" => (!local).then(|| EditValue::String(self.user.clone())),
            "auth" => (!local).then(|| EditValue::StringList(self.auth_names())),
            "identity_file" => (!local).then(|| optional(&self.identity_file)).flatten(),
            _ => None,
        }
    }

    /// As chaves do tipo, na ordem em que se escrevem. O `id` vai primeiro.
    fn keys(&self) -> &'static [&'static str] {
        match self.kind {
            ProfileKind::Local => &["id", "name", "badge", "program", "args", "env", "cwd"],
            ProfileKind::Ssh => &[
                "id",
                "name",
                "type",
                "badge",
                "host",
                "port",
                "user",
                "auth",
                "identity_file",
            ],
        }
    }

    fn entries(&self) -> Vec<(String, EditValue)> {
        self.keys()
            .iter()
            .filter_map(|key| self.value_of(key).map(|value| ((*key).to_owned(), value)))
            .collect()
    }

    /// O que o parser diz deste perfil sozinho: o primeiro problema, por campo.
    /// O perfil é escrito num texto de prova e lido de volta -- a regra é a do
    /// ADR-0070 §1 e do ADR-0072 §6, uma só.
    fn problems(&self) -> BTreeMap<Field, ProfileProblemKind> {
        let Ok(array) = KeyPath::parse(LIST) else {
            return BTreeMap::new();
        };
        let Ok(doc) = ConfigDocument::parse("") else {
            return BTreeMap::new();
        };
        let Ok(text) = doc.apply(&[Edit::AppendTable {
            array,
            entries: self.entries(),
        }]) else {
            return BTreeMap::new();
        };
        let mut found = BTreeMap::new();
        if let Ok(parsed) = porecatu_config::parse_checked(&text) {
            for problem in parsed.profile_problems {
                if let Some(field) = field_of(&problem.kind) {
                    found.entry(field).or_insert(problem.kind);
                }
            }
        }
        found
    }

    /// The parser's problems plus a repeated `id` (only visible against the
    /// other profiles, `others` being their ids). One rule for the draft's
    /// profiles and for the editor's copy.
    pub(crate) fn problems_against(&self, others: &[&str]) -> BTreeMap<Field, ProfileProblemKind> {
        let mut found = self.problems();
        if !self.id.is_empty() && others.contains(&self.id.as_str()) {
            found.insert(
                Field::Id,
                ProfileProblemKind::IdDuplicate {
                    id: self.id.clone(),
                },
            );
        }
        found
    }

    /// The text fields in the order the form shows them: the authentication
    /// choice sits between the user and the key file (ADR-0080 §4).
    pub(crate) fn form_order(&self) -> Vec<Field> {
        let mut order = self.fields().to_vec();
        if let Some(at) = order.iter().position(|f| *f == Field::IdentityFile) {
            order.insert(at, Field::Auth);
        }
        order
    }

    /// Writes a text field. Renaming a profile whose `id` nobody edited takes
    /// the `id` along (`id_follows_name`, `taken` being the other profiles'
    /// ids); editing the `id` drops that link. A field the kind does not show
    /// is ignored.
    pub(crate) fn set_text(
        &mut self,
        field: Field,
        value: &str,
        id_follows_name: &mut bool,
        taken: &[&str],
    ) {
        if !self.fields().contains(&field) {
            return;
        }
        let value = value.to_owned();
        match field {
            Field::Id => {
                self.id = value;
                *id_follows_name = false;
            }
            Field::Name => {
                self.name = value;
                if *id_follows_name {
                    self.id = id_from_name(&self.name, taken);
                }
            }
            Field::Badge => self.badge = value,
            Field::Program => self.program = value,
            Field::Cwd => self.cwd = value,
            Field::Host => self.host = value,
            Field::Port => self.port = value,
            Field::User => self.user = value,
            Field::IdentityFile => self.identity_file = value,
            Field::Auth => {}
        }
    }

    pub(crate) fn set_args(&mut self, args: Vec<String>) {
        if self.kind == ProfileKind::Local {
            self.args = args;
        }
    }

    pub(crate) fn set_env(&mut self, env: Vec<(String, String)>) {
        if self.kind == ProfileKind::Local {
            self.env = env;
        }
    }

    /// Picks one of the [`AUTH_PRESETS`] (SSH only).
    pub(crate) fn set_auth_preset(&mut self, preset: usize) {
        if let Some(methods) = AUTH_PRESETS.get(preset)
            && self.kind == ProfileKind::Ssh
        {
            self.auth = methods.to_vec();
        }
    }

    /// The arguments or environment list as the screen shows it: one argument
    /// per row (the second text empty), or name and value.
    pub(crate) fn list_rows(&self, kind: ListKind) -> Vec<(String, String)> {
        match kind {
            ListKind::Args => self
                .args
                .iter()
                .map(|arg| (arg.clone(), String::new()))
                .collect(),
            ListKind::Env => self.env.clone(),
        }
    }

    fn set_list_rows(&mut self, kind: ListKind, rows: Vec<(String, String)>) {
        if self.kind != ProfileKind::Local {
            return;
        }
        match kind {
            ListKind::Args => self.args = rows.into_iter().map(|(arg, _)| arg).collect(),
            ListKind::Env => self.env = rows,
        }
    }

    /// Changes one text of a list item; `false` if nothing changed.
    pub(crate) fn set_list_text(
        &mut self,
        kind: ListKind,
        item: usize,
        second: bool,
        text: &str,
    ) -> bool {
        let mut rows = self.list_rows(kind);
        let Some(row) = rows.get_mut(item) else {
            return false;
        };
        let field = if second { &mut row.1 } else { &mut row.0 };
        if field == text {
            return false;
        }
        *field = text.to_owned();
        self.set_list_rows(kind, rows);
        true
    }

    /// Appends an empty item and returns its index.
    pub(crate) fn list_add(&mut self, kind: ListKind) -> Option<usize> {
        if self.kind != ProfileKind::Local {
            return None;
        }
        let mut rows = self.list_rows(kind);
        rows.push((String::new(), String::new()));
        let at = rows.len() - 1;
        self.set_list_rows(kind, rows);
        Some(at)
    }

    pub(crate) fn list_remove(&mut self, kind: ListKind, item: usize) -> bool {
        let mut rows = self.list_rows(kind);
        if item >= rows.len() {
            return false;
        }
        rows.remove(item);
        self.set_list_rows(kind, rows);
        true
    }

    /// Item `item` of the environment has an empty name (with a value) or a
    /// repeated one: the field border goes to Error and the profile is not
    /// written until fixed.
    pub(crate) fn env_item_invalid(&self, item: usize) -> bool {
        let rows = self.list_rows(ListKind::Env);
        let Some((name, value)) = rows.get(item) else {
            return false;
        };
        if name.is_empty() {
            return !value.is_empty();
        }
        rows.iter().filter(|(other, _)| other == name).count() > 1
    }

    /// The first invalid item of the environment, if any.
    pub(crate) fn env_problem(&self) -> Option<usize> {
        let count = self.list_rows(ListKind::Env).len();
        (0..count).find(|item| self.env_item_invalid(*item))
    }

    /// What the "Esquecer" button of this profile wipes from the vault: the
    /// password of the destination, or the passphrase of the key file (none,
    /// without a file). Only an SSH profile has them.
    pub(crate) fn forget_target(&self, kind: SecretKind) -> Option<Forget> {
        let (destination, key) = ProfilesDraft::ssh_targets(self)?;
        match kind {
            SecretKind::Password => Some(Forget::Password(destination)),
            SecretKind::Passphrase => (!key.trim().is_empty())
                .then(|| Forget::Passphrase(porecatu_config::expand_tilde(&key))),
        }
    }
}

/// O campo a que um problema do parser se refere.
fn field_of(kind: &ProfileProblemKind) -> Option<Field> {
    Some(match kind {
        ProfileProblemKind::IdInvalid
        | ProfileProblemKind::IdReserved
        | ProfileProblemKind::IdDuplicate { .. } => Field::Id,
        ProfileProblemKind::NameEmpty => Field::Name,
        ProfileProblemKind::BadgeEmpty | ProfileProblemKind::BadgeTooLong => Field::Badge,
        ProfileProblemKind::ProgramEmpty => Field::Program,
        ProfileProblemKind::HostInvalid => Field::Host,
        ProfileProblemKind::UserInvalid => Field::User,
        ProfileProblemKind::PortInvalid => Field::Port,
        ProfileProblemKind::AuthInvalid => Field::Auth,
        ProfileProblemKind::KeyWithoutIdentityFile => Field::IdentityFile,
        // Nenhum destes nasce de um formulário que escreve só as chaves do tipo.
        ProfileProblemKind::TypeUnknown
        | ProfileProblemKind::SshFieldNotAllowed { .. }
        | ProfileProblemKind::DefaultUnknown { .. } => return None,
    })
}

/// O identificador gerado a partir do nome (ADR-0070 §1: `^[a-z0-9][a-z0-9_-]{0,31}$`):
/// minúsculas, o que não cabe vira `-`, sem `-` ou `_` na frente, até 32
/// caracteres, e único entre os `taken` (com `-2`, `-3`, ...).
pub(crate) fn id_from_name(name: &str, taken: &[&str]) -> String {
    let mut id: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Junta os `-` seguidos e tira os da frente: precisa começar por [a-z0-9].
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let id = id.trim_start_matches(['-', '_']).trim_end_matches('-');
    let base: String = if id.is_empty() {
        "profile".to_owned()
    } else {
        id.chars().take(32).collect()
    };
    let is_free = |candidate: &str| !taken.contains(&candidate);
    if is_free(&base) {
        return base;
    }
    for n in 2.. {
        let suffix = format!("-{n}");
        let room = 32 - suffix.len();
        let candidate: String = base.chars().take(room).collect::<String>() + &suffix;
        if is_free(&candidate) {
            return candidate;
        }
    }
    base
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    /// Stable identity inside this draft: never the position, never reused, and
    /// untouched by `rebase`/`delete` (unlike `origin`, which `rebase` moves).
    /// It is what an open profile editor points at (ADR-0080 §6).
    key: u64,
    /// O `id` que o perfil tem no arquivo; `None` num perfil criado na tela.
    origin: Option<String>,
    form: Form,
    /// O `id` ainda acompanha o nome (perfil novo, `id` nunca editado à mão).
    id_follows_name: bool,
}

/// Qual segredo salvo um botão "Esquecer" fala.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretKind {
    Password,
    Passphrase,
}

/// O que apagar no cofre quando o Salvar levar a exclusão (RF-20.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Forget {
    Password(Destination),
    Passphrase(PathBuf),
}

/// O que o diálogo de excluir precisa saber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeleteInfo {
    pub(crate) name: String,
    /// O perfil é SSH e **nenhum outro** aponta para o mesmo destino ou para o
    /// mesmo arquivo de chave: a caixa "Apagar também a senha e a passphrase
    /// salvas" aparece, marcada (RF-20.4).
    pub(crate) offers_forget: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProfilesDraft {
    base: Vec<(String, Form)>,
    items: Vec<Item>,
    /// Os `id` do arquivo que a tela tirou.
    removed: Vec<String>,
    forgets: Vec<Forget>,
    selected: Option<usize>,
    /// The `key` the next item gets.
    next_key: u64,
}

impl ProfilesDraft {
    pub(crate) fn new(list: &[ProfileDef]) -> Self {
        let base: Vec<(String, Form)> = list
            .iter()
            .map(|def| (def.id.clone(), Form::from_def(def)))
            .collect();
        Self::from_base(&base, 0)
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn form(&self, index: usize) -> Option<&Form> {
        self.items.get(index).map(|item| &item.form)
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn select(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected = Some(index);
        }
    }

    /// O perfil foi criado na tela e ainda não está no arquivo.
    pub(crate) fn is_new(&self, index: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|item| item.origin.is_none())
    }

    fn taken_ids(&self) -> Vec<&str> {
        self.items
            .iter()
            .map(|item| item.form.id.as_str())
            .collect()
    }

    /// Acrescenta um perfil local (`name` e `badge` iniciais vêm de quem chama,
    /// que os tira do catálogo de idiomas) e o escolhe.
    pub(crate) fn add_local(&mut self, name: &str, badge: &str) -> usize {
        let id = id_from_name(name, &self.taken_ids());
        self.push_new(Form::new_local(id, name, badge), true)
    }

    /// Acrescenta um perfil SSH e o escolhe.
    pub(crate) fn add_ssh(&mut self, name: &str) -> usize {
        let id = id_from_name(name, &self.taken_ids());
        self.push_new(Form::new_ssh(id, name), true)
    }

    fn push_new(&mut self, form: Form, id_follows_name: bool) -> usize {
        let key = self.next_key;
        self.next_key += 1;
        self.items.push(Item {
            key,
            origin: None,
            form,
            id_follows_name,
        });
        let index = self.items.len() - 1;
        self.selected = Some(index);
        index
    }

    /// Escreve um campo de texto. Mudar o nome de um perfil **novo**, cujo `id`
    /// ninguém editou, leva o `id` junto (ADR-0075 §6); editar o `id` solta essa
    /// ligação. Campo que o tipo não mostra é ignorado.
    pub(crate) fn set_text(&mut self, index: usize, field: Field, value: &str) {
        let others: Vec<String> = self
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, item)| item.form.id.clone())
            .collect();
        let Some(item) = self.items.get_mut(index) else {
            return;
        };
        let taken: Vec<&str> = others.iter().map(String::as_str).collect();
        item.form
            .set_text(field, value, &mut item.id_follows_name, &taken);
    }

    pub(crate) fn set_args(&mut self, index: usize, args: Vec<String>) {
        if let Some(item) = self.items.get_mut(index) {
            item.form.set_args(args);
        }
    }

    pub(crate) fn set_env(&mut self, index: usize, env: Vec<(String, String)>) {
        if let Some(item) = self.items.get_mut(index) {
            item.form.set_env(env);
        }
    }

    /// Escolhe um dos [`AUTH_PRESETS`] (só num perfil SSH).
    pub(crate) fn set_auth_preset(&mut self, index: usize, preset: usize) {
        if let Some(item) = self.items.get_mut(index) {
            item.form.set_auth_preset(preset);
        }
    }

    /// Os problemas do perfil, por campo: os do parser mais o `id` repetido
    /// (que só se vê olhando os outros).
    pub(crate) fn problems(&self, index: usize) -> BTreeMap<Field, ProfileProblemKind> {
        let Some(item) = self.items.get(index) else {
            return BTreeMap::new();
        };
        let others: Vec<&str> = self
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, other)| other.form.id.as_str())
            .collect();
        item.form.problems_against(&others)
    }

    pub(crate) fn is_valid(&self, index: usize) -> bool {
        self.problems(index).is_empty() && self.env_problem(index).is_none()
    }

    /// A lista de argumentos ou de ambiente do perfil `index`, como a tela a
    /// mostra: um argumento por linha (o segundo texto vazio), ou nome e valor.
    pub(crate) fn list_rows(&self, index: usize, kind: ListKind) -> Vec<(String, String)> {
        self.items
            .get(index)
            .map(|item| item.form.list_rows(kind))
            .unwrap_or_default()
    }

    /// Muda um texto de um item de lista; `false` se não mudou nada.
    pub(crate) fn set_list_text(
        &mut self,
        index: usize,
        kind: ListKind,
        item: usize,
        second: bool,
        text: &str,
    ) -> bool {
        self.items
            .get_mut(index)
            .is_some_and(|it| it.form.set_list_text(kind, item, second, text))
    }

    /// Acrescenta um item vazio e devolve o índice dele.
    pub(crate) fn list_add(&mut self, index: usize, kind: ListKind) -> Option<usize> {
        self.items.get_mut(index)?.form.list_add(kind)
    }

    pub(crate) fn list_remove(&mut self, index: usize, kind: ListKind, item: usize) -> bool {
        self.items
            .get_mut(index)
            .is_some_and(|it| it.form.list_remove(kind, item))
    }

    /// O item `item` da lista de ambiente tem nome vazio (com valor) ou repetido:
    /// a borda do campo fica em Erro, e o perfil não é gravado até corrigir.
    pub(crate) fn env_item_invalid(&self, index: usize, item: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|it| it.form.env_item_invalid(item))
    }

    /// Algum item do ambiente é inválido.
    pub(crate) fn env_problem(&self, index: usize) -> Option<usize> {
        self.items.get(index)?.form.env_problem()
    }

    /// Algum perfil tem problema. Não bloqueia o Salvar (ADR-0075 §6): só aquele
    /// perfil deixa de ser gravado.
    pub(crate) fn has_invalid(&self) -> bool {
        (0..self.items.len()).any(|index| !self.is_valid(index))
    }

    /// Os `id` dos perfis válidos, na ordem da lista: as opções de
    /// `[profiles] default`.
    pub(crate) fn valid_ids(&self) -> Vec<String> {
        (0..self.items.len())
            .filter(|index| self.is_valid(*index))
            .map(|index| self.items[index].form.id.clone())
            .collect()
    }

    /// O destino de senha e o arquivo de chave de um perfil SSH.
    fn ssh_targets(form: &Form) -> Option<(Destination, String)> {
        (form.kind == ProfileKind::Ssh).then(|| {
            let port = form.port.trim().parse::<u16>().unwrap_or(0);
            (
                Destination::new(form.user.clone(), &form.host, port),
                form.identity_file.clone(),
            )
        })
    }

    /// O que o botão "Esquecer" de `index` apaga do cofre: a senha do destino,
    /// ou a passphrase do arquivo de chave (nenhuma, sem arquivo informado).
    pub(crate) fn forget_target(&self, index: usize, kind: SecretKind) -> Option<Forget> {
        self.items.get(index)?.form.forget_target(kind)
    }

    /// Os destinos SSH que o rascunho tem agora, com o que o formulário diz:
    /// o que a importação compara para marcar "já importado" (RF-20.7). Um
    /// perfil com porta que não é número não entra.
    pub(crate) fn ssh_destinations(&self) -> Vec<SshProfile> {
        self.items
            .iter()
            .filter(|item| item.form.kind == ProfileKind::Ssh)
            .filter_map(|item| {
                Some(SshProfile {
                    host: item.form.host.clone(),
                    port: item.form.port.trim().parse().ok()?,
                    user: item.form.user.clone(),
                    auth: item.form.auth.clone(),
                    identity_file: item.form.identity_file.clone(),
                })
            })
            .collect()
    }

    /// Acrescenta um perfil SSH por candidato, na ordem dada, e escolhe o
    /// último. O nome é o do bloco `Host`; o `id` sai dele pela regra de sempre
    /// (`-2` em colisão). A importação é uma cópia: nada do arquivo fica ligado
    /// ao perfil.
    pub(crate) fn add_imported(&mut self, candidates: &[&SshImportCandidate]) {
        for candidate in candidates {
            let id = id_from_name(&candidate.alias, &self.taken_ids());
            let mut form = Form::new_ssh(id, &candidate.alias);
            form.host = candidate.host.clone();
            form.port = candidate.port.to_string();
            form.user = candidate.user.clone();
            form.identity_file = candidate.identity_file.clone();
            self.push_new(form, true);
        }
    }

    /// O diálogo de excluir `index` (RF-20.4).
    pub(crate) fn delete_info(&self, index: usize) -> Option<DeleteInfo> {
        let item = self.items.get(index)?;
        let offers_forget = Self::ssh_targets(&item.form).is_some_and(|(destination, key)| {
            !self.items.iter().enumerate().any(|(i, other)| {
                i != index
                    && Self::ssh_targets(&other.form).is_some_and(
                        |(other_destination, other_key)| {
                            other_destination == destination
                                || (!key.is_empty() && other_key == key)
                        },
                    )
            })
        });
        Some(DeleteInfo {
            name: item.form.name.clone(),
            offers_forget,
        })
    }

    /// Tira o perfil da lista. Com `forget` e a caixa disponível, a senha e a
    /// passphrase **entram na fila do Salvar**: nada é apagado do cofre agora.
    pub(crate) fn delete(&mut self, index: usize, forget: bool) {
        let Some(info) = self.delete_info(index) else {
            return;
        };
        let item = self.items.remove(index);
        if forget
            && info.offers_forget
            && let Some((destination, key)) = Self::ssh_targets(&item.form)
        {
            self.forgets.push(Forget::Password(destination));
            if !key.is_empty() {
                self.forgets
                    .push(Forget::Passphrase(porecatu_config::expand_tilde(&key)));
            }
        }
        if let Some(origin) = item.origin {
            self.removed.push(origin);
        }
        self.selected = match self.items.len() {
            0 => None,
            len => Some(index.min(len - 1)),
        };
    }

    /// O `Form` do arquivo que `item` já é, pelo que seria escrito (a mesma
    /// régua de `changed`), e não pelo texto cru.
    fn written_as(base: &Form, item: &Form) -> bool {
        item.keys()
            .iter()
            .all(|key| base.value_of(key) == item.value_of(key))
    }

    fn base_form(&self, origin: &str) -> Option<&Form> {
        self.base
            .iter()
            .find(|(id, _)| id == origin)
            .map(|(_, form)| form)
    }

    /// O perfil difere do que está no arquivo (ou é novo).
    fn changed(&self, item: &Item) -> bool {
        match &item.origin {
            None => true,
            // Pelo que seria escrito, não pelo texto cru: o item vazio de uma
            // lista não é alteração.
            Some(origin) => self
                .base_form(origin)
                .is_none_or(|base| !Self::written_as(base, &item.form)),
        }
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.pending_count() > 0 || !self.forgets.is_empty()
    }

    /// Quantos perfis têm alteração pendente: os criados, os editados e os
    /// excluídos.
    pub(crate) fn pending_count(&self) -> usize {
        self.removed.len() + self.items.iter().filter(|item| self.changed(item)).count()
    }

    /// O perfil `index` tem alteração pendente (a lista o marca).
    pub(crate) fn is_pending(&self, index: usize) -> bool {
        self.items.get(index).is_some_and(|item| self.changed(item))
    }

    /// As edições do Salvar: excluir, acrescentar, editar chave a chave, **tudo
    /// por `id`**. Perfil inválido não entra.
    pub(crate) fn edits(&self) -> Vec<Edit> {
        let Ok(array) = KeyPath::parse(LIST) else {
            return Vec::new();
        };
        let mut out: Vec<Edit> = self
            .removed
            .iter()
            .map(|id| Edit::RemoveTable {
                array: array.clone(),
                id: id.clone(),
            })
            .collect();
        for (index, item) in self.items.iter().enumerate() {
            if !self.changed(item) || !self.is_valid(index) {
                continue;
            }
            match &item.origin {
                None => out.push(Edit::AppendTable {
                    array: array.clone(),
                    entries: item.form.entries(),
                }),
                Some(origin) => {
                    let Some(base) = self.base_form(origin) else {
                        continue;
                    };
                    // `id` por último: as outras edições ainda o acham pelo antigo.
                    let mut renamed = None;
                    for key in item.form.keys() {
                        let (before, after) = (base.value_of(key), item.form.value_of(key));
                        if before == after {
                            continue;
                        }
                        match after {
                            Some(value) if *key == "id" => renamed = Some(value),
                            Some(value) => out.push(Edit::SetInTable {
                                array: array.clone(),
                                id: origin.clone(),
                                key: (*key).to_owned(),
                                value,
                            }),
                            None => out.push(Edit::RemoveInTable {
                                array: array.clone(),
                                id: origin.clone(),
                                key: (*key).to_owned(),
                            }),
                        }
                    }
                    if let Some(value) = renamed {
                        out.push(Edit::SetInTable {
                            array: array.clone(),
                            id: origin.clone(),
                            key: "id".to_owned(),
                            value,
                        });
                    }
                }
            }
        }
        out
    }

    /// Descarta tudo o que é pendente: a lista volta ao arquivo.
    pub(crate) fn discard(&mut self) {
        let selected = self.selected;
        // Keys keep counting: an editor opened before the discard must never
        // land on a profile that got its old key.
        *self = Self::from_base(&self.base, self.next_key);
        self.selected = selected.filter(|index| *index < self.items.len());
        if self.selected.is_none() && !self.items.is_empty() {
            self.selected = Some(0);
        }
    }

    /// The draft as the file has it, the items keyed from `first_key` on.
    fn from_base(base: &[(String, Form)], first_key: u64) -> Self {
        let items = base
            .iter()
            .zip(first_key..)
            .map(|((id, form), key)| Item {
                key,
                origin: Some(id.clone()),
                form: form.clone(),
                id_follows_name: false,
            })
            .collect::<Vec<_>>();
        Self {
            selected: (!items.is_empty()).then_some(0),
            base: base.to_vec(),
            next_key: first_key + items.len() as u64,
            items,
            removed: Vec::new(),
            forgets: Vec::new(),
        }
    }

    // --- Profile editor (ADR-0080 §6) -------------------------------------
    //
    // The editor works on a copy; the draft does not change while it is open,
    // and cancelling is dropping it. Everything below reuses the draft's own
    // rules (`Form`'s setters and `problems_against`): there is no second
    // validation.

    /// The ids of the profiles the editor's copy is compared against: every
    /// profile of the draft but its source (pending ones included).
    pub(super) fn other_ids(&self, target: Target) -> Vec<&str> {
        self.items
            .iter()
            .filter(|item| Some(item.key) != target.key())
            .map(|item| item.form.id.as_str())
            .collect()
    }

    /// Opens an editor on the profile at `index`, which is remembered by its
    /// key, never by the position.
    pub(crate) fn open_editor(&self, index: usize) -> Option<ProfileEditor> {
        let item = self.items.get(index)?;
        Some(ProfileEditor::from_parts(
            Target::Existing { key: item.key },
            item.form.clone(),
            item.id_follows_name,
        ))
    }

    /// Opens an editor on a new local profile (`name` and `badge` come from the
    /// catalog, as in [`Self::add_local`]). Nothing enters the draft until OK.
    pub(crate) fn open_editor_new_local(&self, name: &str, badge: &str) -> ProfileEditor {
        let id = id_from_name(name, &self.taken_ids());
        ProfileEditor::from_parts(Target::New, Form::new_local(id, name, badge), true)
    }

    /// Opens an editor on a new SSH profile.
    pub(crate) fn open_editor_new_ssh(&self, name: &str) -> ProfileEditor {
        let id = id_from_name(name, &self.taken_ids());
        ProfileEditor::from_parts(Target::New, Form::new_ssh(id, name), true)
    }

    /// What is wrong with the editor's copy **as if it were in the draft**: the
    /// parser's problems, the environment's, and an `id` repeated against any
    /// other profile.
    pub(crate) fn editor_problems(&self, editor: &ProfileEditor) -> EditorProblems {
        let form = editor.form();
        let fields = form.problems_against(&self.other_ids(editor.target()));
        let env_item = form.env_problem();
        let first = form
            .form_order()
            .into_iter()
            .find(|field| fields.contains_key(field))
            .map(Focus::Field)
            .or(env_item.map(Focus::EnvItem));
        EditorProblems {
            fields,
            env_item,
            first,
        }
    }

    /// OK: with problems, returns them and changes nothing; without, the copy
    /// replaces its source (which stays where it is, pending) or, for a new
    /// profile -- or one whose source left the draft by a reload or a deletion
    /// -- is appended at the end as new. Returns the profile's index, now the
    /// chosen one. Nothing is written to disk.
    pub(crate) fn apply_editor(&mut self, editor: &ProfileEditor) -> Result<usize, EditorProblems> {
        let problems = self.editor_problems(editor);
        if !problems.is_empty() {
            return Err(problems);
        }
        let form = editor.form().clone();
        let source = editor
            .target()
            .key()
            .and_then(|key| self.items.iter().position(|item| item.key == key));
        let index = match source {
            Some(index) => {
                let item = &mut self.items[index];
                item.form = form;
                item.id_follows_name = editor.id_follows_name();
                index
            }
            None => self.push_new(form, editor.id_follows_name()),
        };
        self.selected = Some(index);
        Ok(index)
    }

    /// O Salvar deu certo: `list` é o arquivo agora. Os perfis válidos que
    /// foram gravados viram o arquivo; os inválidos continuam pendentes. É aqui,
    /// e só aqui, que o cofre esquece o que a exclusão pediu: `forget` roda uma
    /// vez por item da fila, e a fila esvazia.
    pub(crate) fn commit(&mut self, list: &[ProfileDef], mut forget: impl FnMut(&Forget)) {
        for target in std::mem::take(&mut self.forgets) {
            forget(&target);
        }
        self.rebase(list);
        self.removed.clear();
    }

    /// O arquivo mudou (o Salvar, ou alguém por fora): `list` é a nova base. O
    /// que o usuário ainda tem de diferente continua pendente.
    pub(crate) fn rebase(&mut self, list: &[ProfileDef]) {
        let base: Vec<(String, Form)> = list
            .iter()
            .map(|def| (def.id.clone(), Form::from_def(def)))
            .collect();
        // Um perfil cujo `id` de origem sumiu do arquivo passa a ser novo: a
        // tela ainda o tem e o Salvar o acrescenta de novo.
        for item in &mut self.items {
            let still_there = item
                .origin
                .as_ref()
                .is_some_and(|origin| base.iter().any(|(id, _)| id == origin));
            if !still_there {
                item.origin = None;
            }
            // Foi gravado: o `id` do formulário é o do arquivo agora.
            if let Some(origin) = &item.origin
                && let Some((_, saved)) = base.iter().find(|(id, _)| id == origin)
                && Self::written_as(saved, &item.form)
            {
                item.id_follows_name = false;
            }
        }
        // Perfil criado e gravado: acha-o pelo `id` na base nova.
        for item in &mut self.items {
            if item.origin.is_none()
                && base
                    .iter()
                    .any(|(id, form)| *id == item.form.id && Self::written_as(form, &item.form))
            {
                item.origin = Some(item.form.id.clone());
                item.id_follows_name = false;
            }
        }
        // Perfil editado com `id` novo e gravado: a origem passa a ser o `id`
        // novo.
        for item in &mut self.items {
            if let Some(origin) = item.origin.clone()
                && origin != item.form.id
                && base
                    .iter()
                    .any(|(id, form)| *id == item.form.id && Self::written_as(form, &item.form))
            {
                item.origin = Some(item.form.id.clone());
            }
        }
        self.removed
            .retain(|id| base.iter().any(|(base_id, _)| base_id == id));
        self.base = base;
        if let Some(selected) = self.selected {
            self.selected = (!self.items.is_empty()).then(|| selected.min(self.items.len() - 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"[[profiles.list]]
id = "wsl"
name = "WSL Ubuntu"
badge = "WSL"
program = "wsl.exe"
args = ["-d", "Ubuntu"]

[[profiles.list]]
id = "prod"
name = "prod-web-01"
type = "ssh"
host = "10.4.1.20"
user = "deploy"
identity_file = "~/.ssh/id_ed25519"

[[profiles.list]]
id = "stage"
name = "stage"
type = "ssh"
host = "10.4.2.30"
user = "deploy"
"#;

    fn list(text: &str) -> Vec<ProfileDef> {
        let parsed = porecatu_config::parse_checked(text).expect("parses");
        assert!(
            parsed.profile_problems.is_empty(),
            "{:?}",
            parsed.profile_problems
        );
        parsed.config.profiles.list
    }

    fn draft() -> ProfilesDraft {
        ProfilesDraft::new(&list(FILE))
    }

    /// Aplica as edições ao arquivo de prova e relê, como o Salvar faria.
    fn saved(draft: &ProfilesDraft) -> (String, Vec<ProfileDef>) {
        let text = ConfigDocument::parse(FILE)
            .unwrap()
            .apply(&draft.edits())
            .expect("the edits apply");
        let parsed = porecatu_config::parse_checked(&text).expect("the text loads");
        (text, parsed.config.profiles.list)
    }

    #[test]
    fn the_list_has_the_file_s_profiles_in_file_order() {
        let draft = draft();
        assert_eq!(draft.len(), 3);
        let names: Vec<&str> = (0..3)
            .map(|i| draft.form(i).unwrap().name.as_str())
            .collect();
        assert_eq!(names, ["WSL Ubuntu", "prod-web-01", "stage"]);
        assert_eq!(draft.selected(), Some(0));
        assert!(!draft.is_dirty());
        assert_eq!(draft.pending_count(), 0);
        assert!(draft.edits().is_empty());
    }

    #[test]
    fn the_type_decides_which_fields_the_form_shows() {
        let draft = draft();
        assert_eq!(draft.form(0).unwrap().fields(), LOCAL_FIELDS);
        assert_eq!(draft.form(1).unwrap().fields(), SSH_FIELDS);
        assert!(!LOCAL_FIELDS.contains(&Field::Host));
        assert!(!SSH_FIELDS.contains(&Field::Program));
    }

    #[test]
    fn the_id_comes_from_the_name_by_the_rule_of_the_adr() {
        assert_eq!(id_from_name("Ubuntu 24.04", &[]), "ubuntu-24-04");
        assert_eq!(id_from_name("  --Meu Servidor!! ", &[]), "meu-servidor");
        assert_eq!(id_from_name("", &[]), "profile");
        assert_eq!(id_from_name("???", &[]), "profile");
        assert_eq!(id_from_name("a_b-C", &[]), "a_b-c");
        let long = id_from_name(&"x".repeat(80), &[]);
        assert_eq!(long.len(), 32);
        assert_eq!(id_from_name("prod", &["prod"]), "prod-2");
        assert_eq!(id_from_name("prod", &["prod", "prod-2"]), "prod-3");
        let taken_long = "x".repeat(32);
        let next = id_from_name(&taken_long, &[taken_long.as_str()]);
        assert_eq!(next.len(), 32);
        assert!(next.ends_with("-2"));
    }

    #[test]
    fn a_new_local_profile_is_invalid_until_it_has_a_program_and_then_it_is_appended() {
        let mut draft = draft();
        let index = draft.add_local("Meu shell", "SH");

        assert!(draft.is_new(index));
        assert_eq!(draft.selected(), Some(index));
        assert_eq!(draft.form(index).unwrap().id, "meu-shell");
        assert!(draft.problems(index).contains_key(&Field::Program));
        assert!(
            draft.edits().is_empty(),
            "an invalid profile is not written"
        );
        assert!(draft.has_invalid());
        assert!(draft.is_dirty());

        draft.set_text(index, Field::Program, "zsh");

        assert!(draft.is_valid(index));
        let (_, profiles) = saved(&draft);
        assert_eq!(profiles.len(), 4);
        let new = profiles.last().unwrap();
        assert_eq!(
            (new.id.as_str(), new.program.as_str(), new.badge.as_str()),
            ("meu-shell", "zsh", "SH")
        );
    }

    #[test]
    fn a_saved_new_ssh_profile_is_no_longer_pending() {
        for preset in 0..AUTH_PRESETS.len() {
            for identity in ["", "~/.ssh/id_ed25519"] {
                let mut draft = draft();
                let index = draft.add_ssh("Homolog");
                draft.set_text(index, Field::Host, "10.9.9.9");
                draft.set_text(index, Field::User, "ops");
                draft.set_text(index, Field::IdentityFile, identity);
                draft.set_auth_preset(index, preset);
                if !draft.is_valid(index) {
                    continue;
                }
                let (_, profiles) = saved(&draft);
                draft.commit(&profiles, |_| {});
                assert!(!draft.is_new(index), "preset {preset}, key {identity:?}");
                assert!(!draft.is_dirty(), "preset {preset}, key {identity:?}");
                assert!(
                    draft.edits().is_empty(),
                    "preset {preset}, key {identity:?}"
                );
            }
        }
    }

    #[test]
    fn a_saved_new_ssh_profile_with_every_field_filled_is_no_longer_pending() {
        for badge in ["", "SSH", "PRD"] {
            for port in ["22", "2222", " 22 "] {
                let mut draft = draft();
                let index = draft.add_ssh("Homolog");
                draft.set_text(index, Field::Name, "Outro nome");
                draft.set_text(index, Field::Badge, badge);
                draft.set_text(index, Field::Port, port);
                draft.set_text(index, Field::Host, "10.9.9.9");
                draft.set_text(index, Field::User, "ops");
                if !draft.is_valid(index) {
                    continue;
                }
                let (_, profiles) = saved(&draft);
                draft.commit(&profiles, |_| {});
                assert!(!draft.is_dirty(), "badge {badge:?}, port {port:?}");
            }
        }
    }

    #[test]
    fn a_new_ssh_profile_starts_with_port_22_and_the_default_auth_order() {
        let mut draft = draft();
        let index = draft.add_ssh("Homolog");
        let form = draft.form(index).unwrap();
        assert_eq!(form.kind, ProfileKind::Ssh);
        assert_eq!(form.port, "22");
        assert_eq!(form.auth_preset(), Some(0));
        // The parser reports one problem at a time, the first: host, then user.
        assert!(draft.problems(index).contains_key(&Field::Host));
        draft.set_text(index, Field::Host, "10.9.9.9");
        assert!(draft.problems(index).contains_key(&Field::User));
        draft.set_text(index, Field::Host, "10.9.9.9");
        draft.set_text(index, Field::User, "ops");

        let (_, profiles) = saved(&draft);
        let created = profiles
            .iter()
            .find(|p| p.id == "homolog")
            .expect("created");
        let ssh = created.ssh.as_ref().expect("an ssh profile");
        assert_eq!(
            (ssh.host.as_str(), ssh.port, ssh.user.as_str()),
            ("10.9.9.9", 22, "ops")
        );
        assert_eq!(ssh.auth, AUTH_PRESETS[0]);
    }

    #[test]
    fn the_type_never_changes_after_creation() {
        let mut draft = draft();
        let local = draft.add_local("L", "L");
        let ssh = draft.add_ssh("S");
        // Every setter, aimed at the wrong kind or any kind: the kind stays.
        for index in [local, ssh] {
            let kind = draft.form(index).unwrap().kind;
            for field in [
                Field::Id,
                Field::Name,
                Field::Badge,
                Field::Program,
                Field::Cwd,
                Field::Host,
                Field::Port,
                Field::User,
                Field::IdentityFile,
            ] {
                draft.set_text(index, field, "x");
            }
            draft.set_args(index, vec!["a".into()]);
            draft.set_env(index, vec![("K".into(), "V".into())]);
            draft.set_auth_preset(index, 2);
            assert_eq!(draft.form(index).unwrap().kind, kind);
        }
        // A field the kind does not show is ignored.
        assert_eq!(draft.form(local).unwrap().host, "");
        assert_eq!(draft.form(ssh).unwrap().program, "");
        assert!(draft.form(ssh).unwrap().args.is_empty());
    }

    #[test]
    fn the_id_of_a_new_profile_follows_the_name_until_someone_edits_it() {
        let mut draft = draft();
        let index = draft.add_ssh("Novo");
        draft.set_text(index, Field::Name, "Servidor de Teste");
        assert_eq!(draft.form(index).unwrap().id, "servidor-de-teste");

        draft.set_text(index, Field::Id, "teste");
        draft.set_text(index, Field::Name, "Outro nome");
        assert_eq!(
            draft.form(index).unwrap().id,
            "teste",
            "edited by hand: left alone"
        );

        // The id of a profile already in the file never moves with its name.
        draft.set_text(0, Field::Name, "WSL Debian");
        assert_eq!(draft.form(0).unwrap().id, "wsl");
    }

    #[test]
    fn editing_one_field_writes_one_edit_found_by_id() {
        let mut draft = draft();
        draft.set_text(1, Field::Port, "2222");

        let edits = draft.edits();
        assert_eq!(edits.len(), 1, "{edits:?}");
        assert!(matches!(
            &edits[0],
            Edit::SetInTable { id, key, value, .. }
                if id == "prod" && key == "port" && *value == EditValue::Integer(2222)
        ));
        assert_eq!(draft.pending_count(), 1);
        assert!(draft.is_pending(1));
        assert!(!draft.is_pending(0));

        // Back to the file's value: no longer pending.
        draft.set_text(1, Field::Port, "22");
        assert!(draft.edits().is_empty());
        assert!(!draft.is_dirty());
    }

    #[test]
    fn clearing_an_optional_field_drops_its_key() {
        let mut draft = draft();
        draft.set_text(1, Field::IdentityFile, "");

        let edits = draft.edits();
        assert!(matches!(
            edits.as_slice(),
            [Edit::RemoveInTable { id, key, .. }] if id == "prod" && key == "identity_file"
        ));
        let (text, _) = saved(&draft);
        assert!(!text.contains("identity_file"));
    }

    #[test]
    fn a_list_and_a_map_are_edited_whole() {
        let mut draft = draft();
        draft.set_args(0, vec!["-d".into(), "Debian".into()]);
        draft.set_env(0, vec![("A".into(), "1".into()), ("B".into(), "2".into())]);

        let (_, profiles) = saved(&draft);
        assert_eq!(profiles[0].args, ["-d", "Debian"]);
        assert_eq!(profiles[0].env.get("B").map(String::as_str), Some("2"));
    }

    #[test]
    fn the_auth_choice_is_a_preset_and_an_unknown_list_is_kept() {
        let mut draft = draft();
        draft.set_auth_preset(2, 3);
        assert_eq!(draft.form(2).unwrap().auth, [SshAuth::Password]);
        assert_eq!(draft.form(2).unwrap().auth_preset(), Some(3));

        // A custom order from the file is not a preset and is left alone.
        let custom = ProfilesDraft::new(&list(
            "[[profiles.list]]\nid=\"c\"\nname=\"c\"\ntype=\"ssh\"\nhost=\"h\"\nuser=\"u\"\nauth=[\"password\",\"agent\"]\n",
        ));
        assert_eq!(custom.form(0).unwrap().auth_preset(), None);
        assert!(custom.edits().is_empty());
    }

    #[test]
    fn each_invalid_field_is_reported_on_that_field() {
        let mut draft = draft();
        draft.set_text(1, Field::Host, "a b");
        draft.set_text(1, Field::Port, "not-a-number");
        draft.set_text(1, Field::User, "");
        draft.set_text(1, Field::Name, "");
        let problems = draft.problems(1);

        // The parser reports the first problem of an item: the screen shows it
        // on its own field, and the others as the user fixes them.
        assert_eq!(problems.len(), 1, "{problems:?}");
        let field = *problems.keys().next().unwrap();
        assert!(matches!(
            field,
            Field::Name | Field::Host | Field::Port | Field::User
        ));

        draft.set_text(1, Field::Name, "ok");
        draft.set_text(1, Field::Host, "10.0.0.1");
        draft.set_text(1, Field::User, "u");
        let problems = draft.problems(1);
        assert!(problems.contains_key(&Field::Port), "{problems:?}");
        draft.set_text(1, Field::Port, "70000");
        assert!(draft.problems(1).contains_key(&Field::Port));
        draft.set_text(1, Field::Port, "22");
        assert!(draft.is_valid(1));
    }

    #[test]
    fn the_bad_id_rules_are_the_parsers() {
        let mut draft = draft();
        for bad in ["", "Has Space", "UPPER", "auto:shell", &"x".repeat(40)] {
            draft.set_text(0, Field::Id, bad);
            assert!(
                draft.problems(0).contains_key(&Field::Id),
                "{bad:?} must be refused"
            );
        }
        draft.set_text(0, Field::Id, "fine_id-1");
        assert!(draft.is_valid(0));
    }

    #[test]
    fn a_repeated_id_is_reported_on_both_profiles() {
        let mut draft = draft();
        draft.set_text(2, Field::Id, "prod");

        assert!(matches!(
            draft.problems(2).get(&Field::Id),
            Some(ProfileProblemKind::IdDuplicate { .. })
        ));
        assert!(draft.problems(1).contains_key(&Field::Id));
        assert!(draft.edits().iter().all(|edit| !matches!(
            edit,
            Edit::SetInTable { key, .. } if key == "id"
        )));
    }

    #[test]
    fn an_invalid_profile_does_not_stop_the_others_from_being_saved() {
        let mut draft = draft();
        draft.set_text(0, Field::Program, ""); // invalid
        draft.set_text(1, Field::Port, "2222"); // valid

        assert!(draft.has_invalid());
        let (_, profiles) = saved(&draft);
        let prod = profiles.iter().find(|p| p.id == "prod").unwrap();
        assert_eq!(prod.ssh.as_ref().unwrap().port, 2222);
        // The invalid one stays as the file had it.
        assert_eq!(profiles[0].program, "wsl.exe");
    }

    #[test]
    fn renaming_an_id_writes_the_id_last_so_the_other_edits_still_find_it() {
        let mut draft = draft();
        draft.set_text(1, Field::Id, "production");
        draft.set_text(1, Field::Port, "2222");

        let edits = draft.edits();
        assert!(matches!(
            edits.last(),
            Some(Edit::SetInTable { id, key, .. }) if id == "prod" && key == "id"
        ));
        let (_, profiles) = saved(&draft);
        let renamed = profiles
            .iter()
            .find(|p| p.id == "production")
            .expect("renamed");
        assert_eq!(renamed.ssh.as_ref().unwrap().port, 2222);
    }

    #[test]
    fn deleting_the_first_and_editing_the_third_hits_the_right_profile() {
        let mut draft = draft();
        draft.delete(0, false);
        // "stage" was the third and is now the second.
        let stage = (0..draft.len())
            .find(|i| draft.form(*i).unwrap().id == "stage")
            .unwrap();
        draft.set_text(stage, Field::Host, "10.4.2.99");

        let (_, profiles) = saved(&draft);
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["prod", "stage"]);
        assert_eq!(profiles[1].ssh.as_ref().unwrap().host, "10.4.2.99");
    }

    #[test]
    fn deleting_a_profile_that_was_never_saved_writes_nothing() {
        let mut draft = draft();
        let index = draft.add_ssh("Temporário");
        draft.set_text(index, Field::Host, "10.8.8.8");
        draft.set_text(index, Field::User, "u");
        draft.delete(index, false);

        assert!(draft.edits().is_empty());
        assert!(!draft.is_dirty());
        assert_eq!(draft.len(), 3);

        // With the box ticked the vault is asked to forget at the next save, but
        // the file is still not touched.
        let index = draft.add_ssh("Temporário");
        draft.set_text(index, Field::Host, "10.8.8.8");
        draft.set_text(index, Field::User, "u");
        draft.delete(index, true);
        assert!(draft.edits().is_empty());
        assert!(draft.is_dirty());
    }

    #[test]
    fn the_forget_button_targets_the_password_and_the_key_passphrase() {
        let draft = draft();
        assert!(matches!(
            draft.forget_target(1, SecretKind::Password),
            Some(Forget::Password(_))
        ));
        assert!(matches!(
            draft.forget_target(1, SecretKind::Passphrase),
            Some(Forget::Passphrase(path)) if path.ends_with("id_ed25519")
        ));
        // No key file: nothing to forget, and not an error. Local: no vault.
        assert_eq!(draft.forget_target(2, SecretKind::Passphrase), None);
        assert_eq!(draft.forget_target(0, SecretKind::Password), None);
        // Forgetting is immediate: nothing becomes pending.
        assert!(draft.edits().is_empty());
    }

    fn imported(alias: &str, host: &str) -> SshImportCandidate {
        SshImportCandidate {
            alias: alias.to_owned(),
            host: host.to_owned(),
            user: "ops".to_owned(),
            port: 2200,
            identity_file: "C:/keys/id_ops".to_owned(),
            status: porecatu_config::SshImportStatus::Importable,
        }
    }

    #[test]
    fn importing_adds_one_pending_ssh_profile_per_candidate_with_a_derived_id() {
        let mut draft = draft();
        let before = draft.len();
        let ids_before = draft.valid_ids();
        // `prod` already exists as an id in the fixture: they get `-2`, `-3`.
        let a = imported("Prod", "a.example");
        let b = imported("prod", "b.example");
        draft.add_imported(&[&a, &b]);
        assert_eq!(draft.len(), before + 2);
        assert_eq!(draft.pending_count(), 2);
        let first = draft.form(before).unwrap();
        assert_eq!(first.kind, ProfileKind::Ssh);
        assert_eq!(first.name, "Prod");
        assert_eq!(
            (
                first.host.as_str(),
                first.port.as_str(),
                first.user.as_str()
            ),
            ("a.example", "2200", "ops")
        );
        assert_eq!(first.identity_file, "C:/keys/id_ops");
        let second = draft.form(before + 1).unwrap();
        assert_eq!(
            (first.id.as_str(), second.id.as_str()),
            ("prod-2", "prod-3")
        );
        assert!(!ids_before.contains(&first.id));
        // The last one imported is the chosen one, and the profiles are valid.
        assert_eq!(draft.selected(), Some(before + 1));
        assert!(draft.is_valid(before) && draft.is_valid(before + 1));
    }

    #[test]
    fn a_saved_ssh_profile_without_badge_is_not_pending_after_the_reload_gives_it_the_default() {
        let mut draft = ProfilesDraft::new(&[]);
        let index = draft.add_ssh("x");
        draft.set_text(index, Field::Host, "h.example");
        draft.set_text(index, Field::User, "u");
        assert!(draft.is_dirty());
        let saved = ProfileDef {
            id: "x".to_owned(),
            name: "x".to_owned(),
            badge: "SSH".to_owned(),
            kind: ProfileKind::Ssh,
            program: String::new(),
            args: Vec::new(),
            env: Default::default(),
            cwd: String::new(),
            ssh: Some(SshProfile {
                host: "h.example".to_owned(),
                port: 22,
                user: "u".to_owned(),
                auth: AUTH_PRESETS[0].to_vec(),
                identity_file: String::new(),
            }),
        };
        draft.commit(&[saved], |_| {});
        assert!(!draft.is_dirty());
        assert!(!draft.is_pending(index));
    }

    #[test]
    fn the_destinations_offered_for_comparison_are_the_ssh_ones_with_a_numeric_port() {
        let mut draft = draft();
        let expected = draft.ssh_destinations().len();
        assert!(expected >= 1);
        let a = imported("x", "x.example");
        draft.add_imported(&[&a]);
        let destinations = draft.ssh_destinations();
        assert_eq!(destinations.len(), expected + 1);
        assert!(
            destinations
                .iter()
                .any(|d| d.host == "x.example" && d.port == 2200 && d.user == "ops")
        );
        let last = draft.len() - 1;
        draft.set_text(last, Field::Port, "abc");
        assert_eq!(draft.ssh_destinations().len(), expected);
    }

    #[test]
    fn the_forget_box_is_offered_only_when_nobody_else_shares_the_destination_or_key() {
        let mut draft = draft();
        // prod: its own host and key; stage: another host, no key.
        assert!(draft.delete_info(1).unwrap().offers_forget);
        assert!(draft.delete_info(2).unwrap().offers_forget);
        // A local profile has nothing in the vault.
        assert!(!draft.delete_info(0).unwrap().offers_forget);

        // Another profile for the same destination: shared, so no box.
        let twin = draft.add_ssh("prod bis");
        draft.set_text(twin, Field::Host, "10.4.1.20");
        draft.set_text(twin, Field::User, "deploy");
        assert!(!draft.delete_info(1).unwrap().offers_forget);
        assert!(!draft.delete_info(twin).unwrap().offers_forget);

        // Same key file, other host: shared too.
        draft.set_text(twin, Field::Host, "10.4.9.9");
        draft.set_text(twin, Field::IdentityFile, "~/.ssh/id_ed25519");
        assert!(!draft.delete_info(1).unwrap().offers_forget);
    }

    #[test]
    fn the_vault_forgets_only_on_save_and_only_what_the_deletion_asked() {
        let mut draft = draft();
        draft.delete(1, true); // prod: has a key file
        draft.delete(1, false); // stage (now second): the box stays unticked

        // Nothing was forgotten by deleting.
        assert!(draft.is_dirty());
        let mut forgotten = Vec::new();
        let after = list("");
        draft.commit(&after, |target| forgotten.push(target.clone()));

        assert_eq!(forgotten.len(), 2, "{forgotten:?}");
        assert_eq!(
            forgotten[0],
            Forget::Password(Destination::new("deploy", "10.4.1.20", 22))
        );
        assert!(matches!(&forgotten[1], Forget::Passphrase(path) if path.ends_with("id_ed25519")));
        // The queue is empty after the save: a second save forgets nothing.
        let mut again = 0;
        draft.commit(&after, |_| again += 1);
        assert_eq!(again, 0);
    }

    #[test]
    fn discard_brings_the_list_back_and_forgets_nothing() {
        let mut draft = draft();
        draft.delete(1, true);
        draft.set_text(0, Field::Name, "Changed");
        draft.add_local("Extra", "EX");
        assert!(draft.is_dirty());

        draft.discard();

        assert_eq!(draft.len(), 3);
        assert_eq!(draft.form(0).unwrap().name, "WSL Ubuntu");
        assert!(!draft.is_dirty());
        assert!(draft.edits().is_empty());
        let mut forgotten = 0;
        draft.commit(&list(FILE), |_| forgotten += 1);
        assert_eq!(forgotten, 0, "a discarded deletion never reaches the vault");
    }

    #[test]
    fn after_the_save_the_saved_profiles_are_the_file_and_the_screen_is_clean() {
        let mut draft = draft();
        draft.set_text(1, Field::Port, "2222");
        let index = draft.add_local("Novo", "NV");
        draft.set_text(index, Field::Program, "fish");
        draft.delete(2, false);
        let (_, written) = saved(&draft);

        draft.commit(&written, |_| {});

        assert!(!draft.is_dirty(), "{} pending", draft.pending_count());
        assert!(draft.edits().is_empty());
        assert!(!draft.is_new(2), "the new profile is in the file now");
        // Editing it afterwards is an edit by its id, not an append.
        draft.set_text(2, Field::Program, "nu");
        assert!(matches!(
            draft.edits().as_slice(),
            [Edit::SetInTable { id, key, .. }] if id == "novo" && key == "program"
        ));
    }

    #[test]
    fn an_invalid_profile_survives_the_save_still_pending() {
        let mut draft = draft();
        draft.set_text(0, Field::Program, "");
        draft.set_text(1, Field::Port, "2222");
        let (_, written) = saved(&draft);

        draft.commit(&written, |_| {});

        assert!(draft.is_pending(0));
        assert!(!draft.is_pending(1));
        assert!(draft.has_invalid());
    }

    #[test]
    fn when_the_file_changes_underneath_what_is_still_different_stays_pending() {
        let mut draft = draft();
        draft.set_text(1, Field::Port, "2222");
        // Somebody else removed "wsl" and renamed "stage" in the file.
        let outside = list(&FILE.replace("name = \"stage\"", "name = \"stage-renamed\""));

        draft.rebase(&outside);

        assert_eq!(draft.form(1).unwrap().port, "2222");
        assert!(draft.is_pending(1));
        assert!(!draft.is_pending(0), "untouched by me, same as the file");
    }

    #[test]
    fn a_profile_removed_from_the_file_by_someone_else_becomes_new_again() {
        let mut draft = draft();
        draft.set_text(0, Field::Name, "Mine");
        let outside = list(
            &FILE
                .split("[[profiles.list]]")
                .skip(2)
                .fold(String::new(), |acc, part| acc + "[[profiles.list]]" + part),
        );

        draft.rebase(&outside);

        assert!(draft.is_new(0), "its origin is gone from the file");
        assert!(matches!(
            draft.edits().first(),
            Some(Edit::AppendTable { .. })
        ));
    }

    #[test]
    fn the_arguments_list_adds_edits_and_removes_items_and_writes_them_in_order() {
        let mut draft = draft();
        let item = draft.list_add(0, ListKind::Args).unwrap();
        assert_eq!(item, 2, "after the two the file has");
        assert!(draft.set_list_text(0, ListKind::Args, item, false, "--login"));
        assert!(
            !draft.set_list_text(0, ListKind::Args, item, false, "--login"),
            "same text"
        );
        assert!(draft.list_remove(0, ListKind::Args, 0));

        let (_, profiles) = saved(&draft);
        assert_eq!(profiles[0].args, ["Ubuntu", "--login"]);
        assert!(!draft.list_remove(0, ListKind::Args, 9));
    }

    #[test]
    fn an_empty_list_item_is_a_placeholder_and_not_a_change() {
        let mut draft = draft();
        draft.list_add(0, ListKind::Args);
        draft.list_add(0, ListKind::Env);

        assert!(draft.edits().is_empty(), "nothing to write yet");
        assert!(!draft.is_dirty());
        assert!(draft.is_valid(0));
    }

    #[test]
    fn the_environment_needs_a_name_and_no_repeats_before_the_profile_is_written() {
        let mut draft = draft();
        let first = draft.list_add(0, ListKind::Env).unwrap();
        draft.set_list_text(0, ListKind::Env, first, true, "value-without-name");
        assert!(draft.env_item_invalid(0, first), "a value with no name");
        assert!(!draft.is_valid(0));
        assert!(draft.edits().is_empty());

        draft.set_list_text(0, ListKind::Env, first, false, "EDITOR");
        assert!(draft.is_valid(0));
        let second = draft.list_add(0, ListKind::Env).unwrap();
        draft.set_list_text(0, ListKind::Env, second, false, "EDITOR");
        assert!(draft.env_item_invalid(0, first) && draft.env_item_invalid(0, second));
        assert_eq!(draft.env_problem(0), Some(first));
        assert!(!draft.is_valid(0));

        draft.list_remove(0, ListKind::Env, second);
        let (_, profiles) = saved(&draft);
        assert_eq!(
            profiles[0].env.get("EDITOR").map(String::as_str),
            Some("value-without-name")
        );
    }

    #[test]
    fn the_lists_only_exist_in_a_local_profile() {
        let mut draft = draft();
        assert_eq!(draft.list_add(1, ListKind::Args), None);
        assert_eq!(draft.list_add(1, ListKind::Env), None);
        assert!(draft.list_rows(1, ListKind::Args).is_empty());
    }

    #[test]
    fn an_ssh_profile_with_no_auth_and_no_key_reads_as_the_default_preset() {
        let draft = draft();
        // "stage" has neither `auth` nor `identity_file`: agent, then password.
        assert_eq!(
            draft.form(2).unwrap().auth,
            [SshAuth::Agent, SshAuth::Password]
        );
        assert_eq!(draft.form(2).unwrap().auth_preset(), Some(0));
        // "prod" has a key file: the effective order includes the key.
        assert_eq!(draft.form(1).unwrap().auth_preset(), Some(0));
        assert!(draft.edits().is_empty(), "reading it changes nothing");
    }

    #[test]
    fn valid_ids_are_the_choices_of_the_default_profile() {
        let mut draft = draft();
        assert_eq!(draft.valid_ids(), ["wsl", "prod", "stage"]);
        draft.set_text(0, Field::Program, "");
        assert_eq!(draft.valid_ids(), ["prod", "stage"]);
    }

    #[test]
    fn selection_moves_and_survives_a_deletion() {
        let mut draft = draft();
        draft.select(2);
        assert_eq!(draft.selected(), Some(2));
        draft.select(99);
        assert_eq!(draft.selected(), Some(2), "out of range is ignored");

        draft.delete(2, false);
        assert_eq!(draft.selected(), Some(1));
        draft.delete(1, false);
        draft.delete(0, false);
        assert_eq!(draft.selected(), None);
        assert_eq!(draft.len(), 0);
    }

    #[test]
    fn an_empty_file_has_an_empty_list_and_appending_creates_the_array() {
        let mut draft = ProfilesDraft::new(&[]);
        assert_eq!(draft.selected(), None);
        let index = draft.add_local("Primeiro", "P1");
        draft.set_text(index, Field::Program, "cmd.exe");

        let text = ConfigDocument::parse("[shell]\nprogram = \"x\"\n")
            .unwrap()
            .apply(&draft.edits())
            .unwrap();
        let parsed = porecatu_config::parse_checked(&text).unwrap();
        assert_eq!(parsed.config.profiles.list.len(), 1);
        assert!(text.starts_with("[shell]\nprogram = \"x\"\n"));
    }

    // --- Profile editor ---------------------------------------------------

    fn forms(draft: &ProfilesDraft) -> Vec<Form> {
        (0..draft.len())
            .map(|i| draft.form(i).unwrap().clone())
            .collect()
    }

    fn ids(draft: &ProfilesDraft) -> Vec<String> {
        (0..draft.len())
            .map(|i| draft.form(i).unwrap().id.clone())
            .collect()
    }

    #[test]
    fn editing_and_cancelling_leaves_the_draft_as_it_was() {
        let mut draft = draft();
        draft.set_text(0, Field::Name, "Mine"); // a pending edit that must survive
        let before = (forms(&draft), draft.pending_count(), draft.selected());

        let mut editor = draft.open_editor(1).unwrap();
        editor.set_text(&draft, Field::Port, "2222");
        editor.set_text(&draft, Field::Id, "other");
        editor.set_auth_preset(3);
        drop(editor); // Cancel

        assert_eq!(
            (forms(&draft), draft.pending_count(), draft.selected()),
            before
        );
        assert_eq!(ids(&draft), ["wsl", "prod", "stage"]);
        assert_eq!(draft.edits().len(), 1, "only the edit made before it");
        assert!(draft.open_editor(9).is_none());
    }

    #[test]
    fn ok_with_a_problem_returns_the_fields_and_changes_nothing() {
        let mut draft = draft();
        let before = forms(&draft);
        let mut editor = draft.open_editor(1).unwrap();
        editor.set_text(&draft, Field::Host, "a b");

        let problems = draft.apply_editor(&editor).unwrap_err();

        assert!(problems.fields.contains_key(&Field::Host), "{problems:?}");
        assert_eq!(problems.first, Some(Focus::Field(Field::Host)));
        assert_eq!(forms(&draft), before);
        assert!(!draft.is_dirty());

        // Fixed, the same editor goes through.
        editor.set_text(&draft, Field::Host, "10.4.1.20");
        assert_eq!(draft.apply_editor(&editor), Ok(1));
    }

    #[test]
    fn the_first_problem_follows_the_order_of_the_form_and_the_environment_comes_last() {
        let draft = draft();
        let mut editor = draft.open_editor(0).unwrap();
        let item = editor.list_add(ListKind::Env).unwrap();
        editor.set_list_text(ListKind::Env, item, true, "value-without-name");
        assert!(editor.env_item_invalid(item));

        let problems = draft.editor_problems(&editor);
        assert!(problems.fields.is_empty());
        assert_eq!(problems.env_item, Some(item));
        assert_eq!(problems.first, Some(Focus::EnvItem(item)));

        // A field problem comes before the environment's.
        editor.set_text(&draft, Field::Program, "");
        assert_eq!(
            draft.editor_problems(&editor).first,
            Some(Focus::Field(Field::Program))
        );

        // SSH order: the authentication sits between the user and the key file.
        let ssh = draft.open_editor(1).unwrap();
        let order = ssh.form().form_order();
        let at = |field| order.iter().position(|f| *f == field).unwrap();
        assert!(at(Field::User) < at(Field::Auth) && at(Field::Auth) < at(Field::IdentityFile));
    }

    #[test]
    fn the_editor_validates_with_the_drafts_own_rules() {
        // The same input, through the draft's own item and through the editor.
        for (field, value) in [
            (Field::Host, "a b"),
            (Field::Port, "70000"),
            (Field::User, ""),
            (Field::Name, ""),
            (Field::Id, "UPPER"),
            (Field::Id, "wsl"), // repeated against another profile
        ] {
            let mut direct = draft();
            direct.set_text(1, field, value);
            let via_editor = {
                let draft = draft();
                let mut editor = draft.open_editor(1).unwrap();
                editor.set_text(&draft, field, value);
                draft.editor_problems(&editor).fields
            };
            assert!(!via_editor.is_empty(), "{field:?} = {value:?}");
            assert_eq!(via_editor, direct.problems(1), "{field:?} = {value:?}");
        }
    }

    #[test]
    fn ok_without_a_problem_changes_only_the_source_profile_which_becomes_pending() {
        let mut draft = draft();
        let mut editor = draft.open_editor(1).unwrap();
        editor.set_text(&draft, Field::Port, "2222");
        let others = (forms(&draft)[0].clone(), forms(&draft)[2].clone());

        assert_eq!(draft.apply_editor(&editor), Ok(1));

        assert_eq!(ids(&draft), ["wsl", "prod", "stage"], "same order");
        assert_eq!(draft.form(1).unwrap().port, "2222");
        assert_eq!((forms(&draft)[0].clone(), forms(&draft)[2].clone()), others);
        assert_eq!(draft.selected(), Some(1));
        assert!(draft.is_pending(1) && !draft.is_pending(0) && !draft.is_pending(2));
        assert_eq!(draft.pending_count(), 1);
        // Nothing was written: the edits are the ones the Save will make.
        let edits = draft.edits();
        assert!(matches!(
            edits.as_slice(),
            [Edit::SetInTable { id, key, .. }] if id == "prod" && key == "port"
        ));
    }

    #[test]
    fn a_new_profile_cancelled_creates_nothing_and_confirmed_is_appended_with_a_derived_id() {
        let mut draft = draft();
        let mut cancelled = draft.open_editor_new_ssh("Homolog");
        cancelled.set_text(&draft, Field::Host, "10.9.9.9");
        cancelled.set_text(&draft, Field::User, "ops");
        assert!(cancelled.is_new());
        drop(cancelled);
        assert_eq!(ids(&draft), ["wsl", "prod", "stage"]);
        assert!(!draft.is_dirty());

        let mut editor = draft.open_editor_new_local("Meu shell", "SH");
        assert_eq!(editor.form().id, "meu-shell");
        assert_eq!(draft.len(), 3, "opening adds nothing");
        editor.set_text(&draft, Field::Program, "zsh");
        editor.set_text(&draft, Field::Name, "Outro shell");
        assert_eq!(editor.form().id, "outro-shell", "the id follows the name");

        assert_eq!(draft.apply_editor(&editor), Ok(3));

        assert_eq!(ids(&draft), ["wsl", "prod", "stage", "outro-shell"]);
        assert!(draft.is_new(3) && draft.is_pending(3));
        assert_eq!(draft.selected(), Some(3));
        let (_, profiles) = saved(&draft);
        assert_eq!(profiles.last().unwrap().program, "zsh");
    }

    #[test]
    fn the_id_of_a_new_profile_avoids_the_other_ones_and_stops_following_once_edited() {
        let mut draft = draft();
        let mut first = draft.open_editor_new_local("Extra", "EX");
        first.set_text(&draft, Field::Program, "sh");
        draft.apply_editor(&first).unwrap();

        // "extra" is taken by a pending profile: the next one gets `-2`.
        let mut second = draft.open_editor_new_local("Extra", "EX");
        assert_eq!(second.form().id, "extra-2");
        second.set_text(&draft, Field::Program, "sh");
        second.set_text(&draft, Field::Id, "mine");
        second.set_text(&draft, Field::Name, "Whatever");
        assert_eq!(second.form().id, "mine", "edited by hand: left alone");
        assert_eq!(draft.apply_editor(&second), Ok(4));
        assert_eq!(ids(&draft), ["wsl", "prod", "stage", "extra", "mine"]);
    }

    #[test]
    fn a_repeated_id_against_a_pending_profile_is_refused() {
        let mut draft = draft();
        let index = draft.add_local("Extra", "EX");
        draft.set_text(index, Field::Program, "sh");
        let before = forms(&draft);

        // An existing profile taking the pending one's id...
        let mut editor = draft.open_editor(0).unwrap();
        editor.set_text(&draft, Field::Id, "extra");
        let problems = draft.apply_editor(&editor).unwrap_err();
        assert!(matches!(
            problems.fields.get(&Field::Id),
            Some(ProfileProblemKind::IdDuplicate { id }) if id == "extra"
        ));
        assert_eq!(problems.first, Some(Focus::Field(Field::Id)));
        assert_eq!(forms(&draft), before);

        // ...and a new profile does the same.
        let mut new = draft.open_editor_new_local("Another", "AN");
        new.set_text(&draft, Field::Program, "sh");
        new.set_text(&draft, Field::Id, "extra");
        assert!(draft.apply_editor(&new).is_err());
        assert_eq!(forms(&draft), before);

        // Its own id is not a repeat of itself.
        let mut itself = draft.open_editor(index).unwrap();
        itself.set_text(&draft, Field::Badge, "XX");
        assert_eq!(draft.apply_editor(&itself), Ok(index));
    }

    #[test]
    fn deleting_an_earlier_profile_with_the_editor_open_and_confirming_hits_the_right_one() {
        let mut draft = draft();
        let mut editor = draft.open_editor(2).unwrap(); // stage
        editor.set_text(&draft, Field::Host, "10.4.2.99");

        draft.delete(0, false); // wsl: stage is the second now

        assert_eq!(draft.apply_editor(&editor), Ok(1));
        assert_eq!(ids(&draft), ["prod", "stage"]);
        assert_eq!(draft.form(1).unwrap().host, "10.4.2.99");
        assert_eq!(draft.form(0).unwrap().host, "10.4.1.20", "prod untouched");
        assert!(!draft.is_new(1), "still the file's profile, not an append");
        let (_, profiles) = saved(&draft);
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["prod", "stage"]);
        assert_eq!(profiles[1].ssh.as_ref().unwrap().host, "10.4.2.99");
    }

    #[test]
    fn a_source_that_left_the_draft_comes_back_as_new_at_the_end() {
        let mut draft = draft();
        let mut editor = draft.open_editor(1).unwrap(); // prod
        editor.set_text(&draft, Field::Port, "2222");

        draft.delete(1, false);
        assert_eq!(ids(&draft), ["wsl", "stage"]);

        assert_eq!(draft.apply_editor(&editor), Ok(2));
        assert_eq!(ids(&draft), ["wsl", "stage", "prod"]);
        assert!(draft.is_new(2), "its origin is gone: the Save appends it");
        assert_eq!(draft.form(2).unwrap().port, "2222");

        // A reload that dropped the source from the file does not drop the item:
        // it turns into a new one, and the editor still finds it by its key.
        let mut draft = ProfilesDraft::new(&list(FILE));
        let mut editor = draft.open_editor(0).unwrap();
        editor.set_text(&draft, Field::Name, "Mine");
        let without_wsl = list(
            &FILE
                .split("[[profiles.list]]")
                .skip(2)
                .fold(String::new(), |acc, part| acc + "[[profiles.list]]" + part),
        );
        draft.rebase(&without_wsl);
        assert_eq!(
            draft.apply_editor(&editor),
            Ok(0),
            "the item is still there"
        );
    }

    #[test]
    fn a_pending_new_profile_reopens_in_place_even_after_an_earlier_deletion() {
        let mut draft = draft();
        let mut first = draft.open_editor_new_local("Extra", "EX");
        first.set_text(&draft, Field::Program, "sh");
        let index = draft.apply_editor(&first).unwrap();
        assert!(draft.is_new(index), "no file origin to find it by");

        let mut again = draft.open_editor(index).unwrap();
        again.set_text(&draft, Field::Program, "bash");
        draft.delete(0, false);
        assert_eq!(draft.apply_editor(&again), Ok(index - 1));

        assert_eq!(ids(&draft), ["prod", "stage", "extra"]);
        assert_eq!(draft.form(2).unwrap().program, "bash");
        assert_eq!(draft.len(), 3, "edited in place, not appended again");
    }

    #[test]
    fn the_editor_lists_and_auth_edit_the_copy_only() {
        let mut draft = draft();
        let mut editor = draft.open_editor(0).unwrap();
        let item = editor.list_add(ListKind::Args).unwrap();
        assert_eq!(item, 2);
        assert!(editor.set_list_text(ListKind::Args, item, false, "--login"));
        assert!(!editor.set_list_text(ListKind::Args, item, false, "--login"));
        assert!(editor.list_remove(ListKind::Args, 0));
        editor.set_env(vec![("A".into(), "1".into())]);
        assert!(!editor.list_remove(ListKind::Args, 9));
        assert_eq!(
            editor.list_rows(ListKind::Args),
            [
                ("Ubuntu".to_owned(), String::new()),
                ("--login".to_owned(), String::new())
            ]
        );
        assert_eq!(
            draft.list_rows(0, ListKind::Args).len(),
            2,
            "draft untouched"
        );
        assert!(draft.edits().is_empty());

        // The lists exist only in a local profile; the preset, only in an SSH one.
        let mut ssh = draft.open_editor(2).unwrap();
        assert_eq!(ssh.list_add(ListKind::Args), None);
        ssh.set_args(vec!["x".into()]);
        assert!(ssh.form().args.is_empty());
        ssh.set_auth_preset(3);
        assert_eq!(ssh.form().auth, [SshAuth::Password]);
        let mut local = draft.open_editor(0).unwrap();
        local.set_auth_preset(3);
        assert!(local.form().auth.is_empty());

        draft.apply_editor(&editor).unwrap();
        let (_, profiles) = saved(&draft);
        assert_eq!(profiles[0].args, ["Ubuntu", "--login"]);
        assert_eq!(profiles[0].env.get("A").map(String::as_str), Some("1"));
    }

    #[test]
    fn an_invalid_profile_from_the_file_stays_in_the_list_and_does_not_stop_the_others() {
        let mut defs = list(FILE);
        defs[0].program.clear(); // hand-edited into an invalid profile
        let mut draft = ProfilesDraft::new(&defs);

        assert_eq!(draft.len(), 3, "still listed");
        assert!(!draft.is_valid(0) && draft.has_invalid());
        assert_eq!(draft.valid_ids(), ["prod", "stage"]);

        let mut editor = draft.open_editor(1).unwrap();
        editor.set_text(&draft, Field::Port, "2222");
        assert_eq!(draft.apply_editor(&editor), Ok(1));
        let edits = draft.edits();
        assert!(matches!(
            edits.as_slice(),
            [Edit::SetInTable { id, key, .. }] if id == "prod" && key == "port"
        ));

        // Opening the invalid one shows its errors, and OK waits for the fix.
        let mut broken = draft.open_editor(0).unwrap();
        assert_eq!(
            draft.apply_editor(&broken).unwrap_err().first,
            Some(Focus::Field(Field::Program))
        );
        assert_eq!(draft.len(), 3);
        broken.set_text(&draft, Field::Program, "wsl.exe");
        assert_eq!(draft.apply_editor(&broken), Ok(0));
        assert!(draft.is_valid(0));
    }
}
