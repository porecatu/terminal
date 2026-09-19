// SPDX-License-Identifier: GPL-3.0-or-later

//! Branch do repositório Git do diretório da aba ativa (ADR-0049), mais as
//! funções puras da sincronização com o remoto (PRD-013, ADR-0052).
//!
//! **Estado da árvore continua fora** -- sujo/limpo, o que está em stage --
//! e é fronteira, não pendência: isso precisa percorrer a árvore, custa
//! ordens de grandeza mais, e nada barato diz quando refazer (ADR-0049 §7).
//! Ahead/behind **não** percorre a árvore -- fala com a rede e caminha o
//! grafo de commits --, e é por isso que ele entra aqui embora estado da
//! árvore não entre (ADR-0052 §2).
//!
//! A leitura de `HEAD` continua sem `git2`, sem processo `git`, sem thread e
//! sem temporizador: é a leitura de um arquivo de ~30 bytes, revalidada por
//! `mtime`. O que fala com processo, thread e rede é só o módulo
//! [`remote_sync`] -- as decisões (parsing da saída do `rev-list`, o
//! intervalo efetivo, quando consultar, o que fazer com um resultado que
//! chega, o rótulo, o vetor de não-interação) são funções puras, sem
//! `Instant::now()` interno; a execução de verdade ([`spawn_query`]) é o
//! único código deste arquivo com I/O de processo, numa thread de vida
//! curta e detached (ADR-0052 §5), no molde de [`crate::reload::watch`].

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// O que `.git/HEAD` diz. As duas formas que aparecem em uso normal --
/// qualquer outra coisa é `None` em [`parse_head`], não um palpite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// `ref: refs/heads/<nome>`.
    Branch(String),
    /// Um SHA solto: `HEAD` destacado. Guarda já encurtado.
    Detached(String),
}

impl Head {
    /// O que a barra exibe -- o nome, ou o SHA curto.
    pub fn label(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
        }
    }
}

/// Quantos hex de um SHA destacado a barra mostra. É o corte que o
/// próprio `git` usa por padrão ao abreviar.
const SHORT_SHA_LEN: usize = 7;

/// Interpreta o conteúdo de `.git/HEAD`. Pura: é o que torna as duas
/// formas testáveis sem repositório nenhum no disco.
///
/// Devolve `None` para qualquer coisa que não bata exatamente com uma
/// delas -- inclusive conteúdo truncado, que é o que se leria ao pegar o
/// arquivo no meio de uma escrita. Esconder o segmento por um frame é
/// melhor que mostrar lixo.
pub fn parse_head(content: &str) -> Option<Head> {
    let line = content.lines().next()?.trim();
    if let Some(reference) = line.strip_prefix("ref:") {
        // O nome da branch é o que vem depois do último `/`: um
        // `refs/heads/feat/x` é a branch `feat/x`, não `x`, então o corte
        // é do prefixo conhecido, não do último separador.
        let reference = reference.trim();
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        if name.is_empty() {
            return None;
        }
        return Some(Head::Branch(name.to_owned()));
    }
    // SHA solto: 40 hex (SHA-1) ou 64 (SHA-256, que o git já aceita).
    let is_sha =
        (line.len() == 40 || line.len() == 64) && line.chars().all(|c| c.is_ascii_hexdigit());
    if is_sha {
        return Some(Head::Detached(line[..SHORT_SHA_LEN].to_owned()));
    }
    None
}

/// Resolve o conteúdo de um `.git` que é **arquivo**, não diretório --
/// a forma que worktree e submódulo usam: `gitdir: <caminho>`.
///
/// Ignorar isto mostraria "sem repositório" dentro de um worktree, que é
/// pior que não mostrar nada (ADR-0049 §2).
fn parse_gitdir_file(content: &str) -> Option<PathBuf> {
    let line = content.lines().next()?.trim();
    let path = line.strip_prefix("gitdir:")?.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Sobe de `start` até a raiz procurando um `.git`, e devolve o
/// diretório de trabalho onde ele foi achado mais o caminho do
/// **`HEAD`** dentro dele -- o primeiro é a chave de repositório que
/// PRD-013/ADR-0052 §3 pede (`-C <repo>` do processo `git`, e o mapa do
/// processo é chaveado por ele: um `cd src/` não é projeto novo), o
/// segundo é o que a branch já usava.
///
/// É o passo caro -- um `stat` por nível --, e por isso [`GitInfo`] só o
/// refaz quando o `cwd` muda.
fn find_head_file(start: &Path) -> Option<(PathBuf, PathBuf)> {
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        let metadata = std::fs::metadata(&dot_git).ok();
        match metadata {
            Some(m) if m.is_dir() => return Some((dir.to_path_buf(), dot_git.join("HEAD"))),
            Some(m) if m.is_file() => {
                // Worktree ou submódulo. O caminho pode ser relativo ao
                // diretório que contém o `.git`.
                let content = std::fs::read_to_string(&dot_git).ok()?;
                let gitdir = parse_gitdir_file(&content)?;
                let gitdir = if gitdir.is_absolute() {
                    gitdir
                } else {
                    dir.join(gitdir)
                };
                return Some((dir.to_path_buf(), gitdir.join("HEAD")));
            }
            _ => {}
        }
    }
    None
}

/// Branch da aba ativa, com o cache que a torna barata de consultar por
/// frame (ADR-0049 §1). Vive em `WindowState`: é sempre a do `cwd` que a
/// barra está exibindo.
#[derive(Debug, Default)]
pub struct GitInfo {
    /// Para qual `cwd` a descoberta abaixo foi feita. Mudou o `cwd`,
    /// redescobre; é a única coisa que dispara o passo caro.
    resolved_for: Option<PathBuf>,
    /// Diretório de trabalho onde o `.git` foi achado -- a chave de
    /// repositório do mapa de sincronização (PRD-013, ADR-0052 §3).
    /// `None` junto com `head_path`, e pela mesma razão: também
    /// cacheado fora de um repositório.
    repo_root: Option<PathBuf>,
    /// Caminho do `HEAD`, ou `None` se não há repositório acima do `cwd`
    /// -- **e o `None` também é cacheado**: fora de um repositório, a
    /// subida de diretórios não se repete a cada frame.
    head_path: Option<PathBuf>,
    /// `mtime` da última leitura. É o que decide se relê.
    read_at: Option<SystemTime>,
    head: Option<Head>,
}

impl GitInfo {
    /// Revalida contra `cwd` e devolve o `HEAD`, se houver -- branch **ou**
    /// destacado, sem achatar a diferença num rótulo. É o degrau que a
    /// etapa 3 precisa (ADR-0052 §7: `HEAD` destacado não é consultado nem
    /// indicado, e distinguir os dois exige mais que o texto).
    ///
    /// Chamada no caminho que monta o conteúdo da barra -- ou seja, só
    /// quando um frame vai ser desenhado. O trabalho no caso comum (mesmo
    /// `cwd`, `HEAD` intocado) é **um `stat`**. Sem `cwd`, nem isso.
    pub fn head(&mut self, cwd: Option<&Path>) -> Option<&Head> {
        let cwd = cwd?;

        if self.resolved_for.as_deref() != Some(cwd) {
            self.resolved_for = Some(cwd.to_path_buf());
            let found = find_head_file(cwd);
            self.repo_root = found.as_ref().map(|(root, _)| root.clone());
            self.head_path = found.map(|(_, head)| head);
            self.read_at = None;
            self.head = None;
        }

        let head_path = self.head_path.as_ref()?;
        let mtime = std::fs::metadata(head_path).and_then(|m| m.modified()).ok();
        if mtime != self.read_at || self.head.is_none() {
            self.read_at = mtime;
            self.head = std::fs::read_to_string(head_path)
                .ok()
                .as_deref()
                .and_then(parse_head);
        }
        self.head.as_ref()
    }

    /// Diretório de trabalho do repositório de `cwd` -- a chave do mapa
    /// de sincronização com o remoto (PRD-013, ADR-0052 §3): estável a
    /// um `cd` dentro do mesmo projeto, porque é o mesmo que
    /// [`Self::head`] já cacheia contra o `cwd`.
    ///
    /// Revalida como [`Self::head`] (mesmo `stat` de sempre); chamar as
    /// duas seguidas para o mesmo `cwd` não dobra o custo.
    pub fn repo_root(&mut self, cwd: Option<&Path>) -> Option<&Path> {
        self.head(cwd);
        self.repo_root.as_deref()
    }
}

/// PRD-013 / ADR-0052: funções puras da sincronização com o remoto --
/// parsing da saída do `rev-list`, intervalo efetivo, quando consultar, o
/// que fazer com um resultado que chega, o rótulo, o vetor de
/// não-interação do processo `git` e a classificação de falha. Consumidas
/// pela thread de consulta abaixo e pelo ciclo de vida do app em
/// `lib.rs` (ADR-0052 §5, §11).
pub(crate) mod remote_sync {
    use super::*;

    /// Piso do intervalo de consulta, em segundos (RF-13.3). Abaixo dele o
    /// `git` seria lançado rápido demais para um repositório de rede.
    const MIN_POLL_INTERVAL_SECS: u64 = 30;

    /// O que `remote_poll_interval_secs` produz de fato (RF-13.2/RF-13.3):
    /// `0` desliga, valor abaixo do piso é elevado com aviso.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct EffectivePollInterval {
        /// `None` quando o recurso está desligado -- nenhum prazo deve
        /// entrar na conta de `schedule_next_wake` nesse caso (ADR-0052
        /// §10).
        pub interval: Option<Duration>,
        /// `true` quando o valor configurado foi elevado ao piso -- o app
        /// informa isso **uma vez** (RF-13.3), não a cada avaliação.
        pub raised_to_floor: bool,
    }

    /// RF-13.2/RF-13.3. Valores acima do piso passam intactos, por maiores
    /// que sejam -- não há teto documentado, só piso.
    pub fn effective_poll_interval(configured_secs: u64) -> EffectivePollInterval {
        if configured_secs == 0 {
            return EffectivePollInterval {
                interval: None,
                raised_to_floor: false,
            };
        }
        if configured_secs < MIN_POLL_INTERVAL_SECS {
            return EffectivePollInterval {
                interval: Some(Duration::from_secs(MIN_POLL_INTERVAL_SECS)),
                raised_to_floor: true,
            };
        }
        EffectivePollInterval {
            interval: Some(Duration::from_secs(configured_secs)),
            raised_to_floor: false,
        }
    }

    /// Parseia a saída de `rev-list --count --left-right @{u}...HEAD`
    /// (ADR-0052 §7). **A ordem é o detalhe que se erra**: em
    /// `@{u}...HEAD`, a esquerda é o que só o upstream tem -- ATRÁS -- e a
    /// direita é o que só o `HEAD` local tem -- À FRENTE. Ordem verificada
    /// no repositório, não deduzida.
    ///
    /// `None` para qualquer coisa que não seja exatamente dois inteiros
    /// separados por espaço/tab numa linha -- vazio, lixo, campo faltando
    /// ou sobrando. Mostrar nada é melhor que mostrar um número inventado.
    pub fn parse_rev_list_count(output: &str) -> Option<(u32, u32)> {
        let line = output.lines().next()?.trim();
        let mut fields = line.split_whitespace();
        let behind: u32 = fields.next()?.parse().ok()?;
        let ahead: u32 = fields.next()?.parse().ok()?;
        if fields.next().is_some() {
            return None;
        }
        Some((behind, ahead))
    }

    /// Estado de consulta de um repositório no mapa do processo (ADR-0052
    /// §3).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum QueryState {
        /// Nenhuma consulta em andamento; pronta para a próxima quando o
        /// intervalo se cumprir.
        #[default]
        Idle,
        /// Processo `git` lançado, resultado ainda não chegou -- não
        /// redispara (RF-13.11). Reaproveitado pela **integração** (etapa
        /// 4, `pull --ff-only`): enquanto ela roda, a `App` marca a mesma
        /// entrada como `InFlight`, o que barra tanto o segundo clique
        /// quanto uma consulta periódica concorrente no mesmo `.git` --
        /// não há um estado próprio de "pull em andamento" porque a regra
        /// dos dois é idêntica (RF-13.11: "consulta ou pull").
        InFlight,
        /// A branch não segue upstream nenhum -- nunca consultada
        /// (RF-13.18, ADR-0052 §7).
        NoUpstream,
        /// Repositório raso -- nunca consultado (ADR-0052 §7): o `fetch`
        /// poderia aprofundá-lo, que é caro e não foi pedido.
        Shallow,
        /// Última tentativa falhou; `attempt` alimenta o recuo progressivo
        /// (ADR-0052 §6).
        Failed { attempt: u32 },
    }

    /// Teto do multiplicador de recuo -- o bastante para não perder uma
    /// consulta a cada intervalo contra uma VPN fora do ar, sem represar a
    /// primeira consulta boa por um tempo absurdo depois que a rede volta.
    const MAX_BACKOFF_MULTIPLIER: u32 = 8;

    /// Recuo progressivo depois de falha (ADR-0052 §6): dobra o intervalo
    /// a cada tentativa, até o teto acima. `pub(crate)` só para o teste de
    /// unidade chegar nele direto; quem decide de fora é
    /// [`is_time_to_query`].
    pub(crate) fn backoff_interval(interval: Duration, attempt: u32) -> Duration {
        let multiplier = 1u32
            .checked_shl(attempt)
            .unwrap_or(u32::MAX)
            .min(MAX_BACKOFF_MULTIPLIER);
        interval.saturating_mul(multiplier)
    }

    /// Decide se é hora de disparar uma nova consulta. `now` e
    /// `last_queried_at` vêm de fora -- nunca `Instant::now()` aqui, mesma
    /// regra do `project_command_timing` em `lib.rs`.
    pub fn is_time_to_query(
        state: QueryState,
        last_queried_at: Option<Instant>,
        now: Instant,
        interval: Duration,
    ) -> bool {
        let effective_interval = match state {
            QueryState::InFlight | QueryState::NoUpstream | QueryState::Shallow => return false,
            QueryState::Idle => interval,
            QueryState::Failed { attempt } => backoff_interval(interval, attempt),
        };
        match last_queried_at {
            None => true,
            Some(last) => now.duration_since(last) >= effective_interval,
        }
    }

    /// Próximo instante em que alguma entrada do mapa precisa ser
    /// reavaliada (ADR-0052 §1/§5/§10) -- `None` com o recurso desligado
    /// (`effective_interval` é `None` só quando `remote_poll_interval_
    /// secs = 0`) ou sem nenhuma entrada pendente. É esta função, pura e
    /// sem `Instant::now()` interno, que torna o item 2 do critério da
    /// §1 verificável: **com `0`, nada entra na conta, mesmo com
    /// repositórios já no mapa.**
    pub fn next_query_deadline<'a>(
        effective_interval: Option<Duration>,
        entries: impl Iterator<Item = &'a RemoteEntry>,
        now: Instant,
    ) -> Option<Instant> {
        let interval = effective_interval?;
        entries
            .filter_map(|entry| match entry.state {
                QueryState::InFlight | QueryState::NoUpstream | QueryState::Shallow => None,
                QueryState::Idle => Some(entry.last_queried_at.map_or(now, |last| last + interval)),
                QueryState::Failed { attempt } => Some(
                    entry
                        .last_queried_at
                        .map_or(now, |last| last + backoff_interval(interval, attempt)),
                ),
            })
            .min()
    }

    /// O que fazer com um resultado que chega da thread (ADR-0052 §3, o
    /// coração desta entrega): ele **não é um comando para mostrar algo**,
    /// é dado chaveado por repositório. Um repositório que não é mais o da
    /// aba ativa não muda nada aqui -- o mapa é por repositório,
    /// independente de quem está sendo exibido, e o resultado é aceito e
    /// guardado de qualquer forma. O que descarta é a branch ter mudado
    /// **enquanto a consulta estava em voo**: o resultado pertence a uma
    /// branch que já não é a atual, e escrevê-lo sob a branch nova
    /// mentiria.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum IncomingResultDecision {
        Store,
        Discard,
    }

    /// `current_branch` é a branch **atual** do repositório no momento em
    /// que o resultado chega (o `mtime` de `.git/HEAD` já detecta o
    /// `checkout`, ADR-0049 §1) -- não a branch da aba que estava ativa
    /// quando a consulta foi disparada.
    pub fn incoming_result_decision(
        queried_branch: &str,
        current_branch: Option<&str>,
    ) -> IncomingResultDecision {
        if current_branch == Some(queried_branch) {
            IncomingResultDecision::Store
        } else {
            IncomingResultDecision::Discard
        }
    }

    /// RF-13.7/RF-13.8/RF-13.9: singular e plural corretos, e o par
    /// divergente sem a palavra "commits" (é o formato que o PRD mostra:
    /// `"2 atrás, 1 à frente"`). `None` quando não há nada a mostrar --
    /// ausência é a resposta, nunca "0 atrás" nem uma versão apagada dele.
    pub fn ahead_behind_label(behind: u32, ahead: u32) -> Option<String> {
        if behind == 0 {
            return None;
        }
        if ahead == 0 {
            let word = if behind == 1 { "commit" } else { "commits" };
            Some(format!("{behind} {word} atrás"))
        } else {
            Some(format!("{behind} atrás, {ahead} à frente"))
        }
    }

    /// RF-13.9/RF-13.12: `pull --ff-only` só funciona sem commits locais à
    /// frente -- com `ahead > 0` ele falharia por definição, e o
    /// indicador não oferece um botão que já sabe que não funciona.
    pub fn can_integrate(ahead: u32) -> bool {
        ahead == 0
    }

    /// Args de `git` para a consulta (ADR-0052 §7): `-C <repo>` como
    /// argumento próprio do processo, nunca formatado numa string de
    /// comando -- é a propriedade que a segurança desta entrega exige.
    /// `@{u}...HEAD` compara o upstream da branch **atual** do
    /// repositório contra ela mesma, então nenhum nome de branch precisa
    /// viajar como texto vindo de lugar nenhum.
    pub fn rev_list_args(repo: &Path) -> Vec<OsString> {
        vec![
            OsString::from("-C"),
            repo.as_os_str().to_owned(),
            OsString::from("rev-list"),
            OsString::from("--count"),
            OsString::from("--left-right"),
            OsString::from("@{u}...HEAD"),
        ]
    }

    /// O vetor de não-interação (ADR-0052 §6) -- **o teste de segurança
    /// desta entrega**. São os quatro canais que, ligados, travariam a
    /// thread para sempre esperando um humano que nunca vai responder;
    /// desligar só três deixa o quarto travar do mesmo jeito.
    pub fn non_interactive_env() -> Vec<(&'static str, &'static str)> {
        vec![
            // Canal 1: prompt de usuário/senha no próprio terminal do
            // `git`.
            ("GIT_TERMINAL_PROMPT", "0"),
            // Canal 2: askpass do git -- string vazia sobrepõe qualquer
            // `core.askpass` herdado da config do usuário, que `git`
            // respeitaria mesmo com o canal 1 desligado.
            ("GIT_ASKPASS", ""),
            // Canal 3: askpass do ssh, e passphrase de chave --
            // `BatchMode=yes` faz o `ssh` falhar em vez de perguntar, por
            // qualquer caminho que ele use para pedir.
            ("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"),
            // Canal 4, o pior: gerenciador de credenciais, que abre
            // **janela gráfica própria** -- não é console, e a flag de
            // supressão de console não a segura.
            ("GCM_INTERACTIVE", "Never"),
        ]
    }

    /// Classificação de uma falha de consulta (ADR-0052 §6/§7, RF-13.19).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum QueryFailure {
        /// `git` não está no sistema -- desliga o recurso pelo resto da
        /// execução e informa uma vez (RF-13.19), sem tentar de novo.
        GitMissing,
        /// Não alcançou o remoto -- entra em recuo progressivo (ADR-0052
        /// §6).
        Network,
        /// Credencial recusada, ou pedida e recusada pelo vetor de
        /// não-interação -- entra em recuo, como falha de rede.
        Auth,
        /// Qualquer outra falha do processo -- também entra em recuo.
        Other,
    }

    /// `spawn_failed_to_start` é o caso em que o próprio `Command::spawn`
    /// falhou (tipicamente `ErrorKind::NotFound`) -- é o único caminho
    /// para `GitMissing`, porque só ele diz que não há `git` nenhum para
    /// rodar. Daí em diante a classificação é heurística sobre o texto
    /// que o `git` escreveu em `stderr`, a mesma classe de heurística que
    /// o projeto já usa noutros lugares (ex. merge de tema por "diferente
    /// do default").
    pub fn classify_failure(spawn_failed_to_start: bool, stderr: &str) -> QueryFailure {
        if spawn_failed_to_start {
            return QueryFailure::GitMissing;
        }
        let lower = stderr.to_lowercase();
        let network_markers = [
            "could not resolve host",
            "could not resolve hostname",
            "unable to access",
            "network is unreachable",
            "connection timed out",
            "timed out",
            "failed to connect",
        ];
        if network_markers.iter().any(|marker| lower.contains(marker)) {
            return QueryFailure::Network;
        }
        let auth_markers = [
            "authentication failed",
            "permission denied",
            "could not read username",
            "could not read password",
            "terminal prompts disabled",
            "invalid credentials",
        ];
        if auth_markers.iter().any(|marker| lower.contains(marker)) {
            return QueryFailure::Auth;
        }
        QueryFailure::Other
    }

    /// Estado de um repositório no mapa do processo (ADR-0052 §3): a
    /// consulta atual, o instante da última tentativa, e a contagem --
    /// junto da branch a que ela pertence, porque um `checkout` no
    /// terminal não pode deixar o número velho na tela sob o nome novo
    /// (RF-13.10).
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct RemoteEntry {
        pub state: QueryState,
        pub last_queried_at: Option<Instant>,
        /// Branch a que `state`/`counts` pertencem -- `None` até a
        /// primeira consulta ser disparada para este repositório.
        pub branch: Option<String>,
        /// `Some((atrás, à frente))` só depois de uma consulta bem
        /// sucedida; `None` em qualquer outro estado (inclusive
        /// `NoUpstream`/`Shallow`/`Failed`, onde não há número a
        /// mostrar).
        pub counts: Option<(u32, u32)>,
    }

    /// O que uma consulta produziu, pronto para o canal de eventos
    /// (ADR-0052 §3) -- **dado chaveado, nunca um comando para mostrar
    /// algo**. Quem recebe só guarda; quem decide o que a barra mostra é
    /// o caminho que monta o conteúdo dela a cada quadro.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RemoteQueryOutcome {
        Counted {
            behind: u32,
            ahead: u32,
        },
        /// ADR-0052 §7: detectado sem consulta de rede nenhuma --
        /// `branch_has_upstream` sobre `.git/config`.
        NoUpstream,
        /// ADR-0052 §7: detectado por um `stat` em `.git/shallow`, sem
        /// consulta de rede.
        Shallow,
        Failed(QueryFailure),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RemoteQueryResult {
        /// Diretório de trabalho do repositório -- a mesma chave de
        /// [`super::GitInfo::repo_root`].
        pub repo: PathBuf,
        /// Branch consultada -- a que estava ativa quando a thread foi
        /// disparada, não necessariamente a atual quando o resultado
        /// chega.
        pub branch: String,
        pub outcome: RemoteQueryOutcome,
    }

    /// O que uma integração produziu (ADR-0052 §7.1, RF-13.14). Ao contrário
    /// de [`RemoteQueryOutcome`], a falha viaja como **texto**, não como
    /// enum classificado -- é a mensagem do `git` que diz o que fazer a
    /// seguir (RF-13.14), e `QueryFailure` foi desenhado para o recuo
    /// progressivo da consulta, não para o que o usuário lê no aviso.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum IntegrationOutcome {
        Success,
        Failed { message: String },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RemoteIntegrationResult {
        /// Mesma chave do mapa do processo -- `super::GitInfo::repo_root`.
        pub repo: PathBuf,
        pub outcome: IntegrationOutcome,
    }

    /// Lê se a branch tem upstream configurado, sem tocar rede (ADR-0052
    /// §7: branch sem upstream não gera consulta nenhuma, nem processo).
    /// Olha `.git/config` por uma seção `[branch "<nome>"]` com `remote`
    /// **e** `merge` -- as duas chaves que `git branch
    /// --set-upstream-to` escreve juntas. Pura sobre o texto do arquivo;
    /// quem lê o arquivo é o chamador.
    ///
    /// Não é um parser de INI completo (sem continuação de linha, sem
    /// escape de aspas) -- suficiente para o que este arquivo de fato
    /// contém, que o `git` escreve sozinho.
    pub fn branch_has_upstream(config_text: &str, branch: &str) -> bool {
        let mut in_section = false;
        let mut has_remote = false;
        let mut has_merge = false;
        for line in config_text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('[') {
                let rest = rest.strip_suffix(']').unwrap_or(rest);
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or("");
                let subsection = parts.next().map(str::trim);
                in_section = name.eq_ignore_ascii_case("branch")
                    && subsection == Some(&*format!("\"{branch}\""));
                continue;
            }
            if !in_section {
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("remote") {
                has_remote |= value.trim_start().starts_with('=');
            } else if let Some(value) = trimmed.strip_prefix("merge") {
                has_merge |= value.trim_start().starts_with('=');
            }
        }
        has_remote && has_merge
    }
}

pub(crate) use remote_sync::*;

/// Intervalo de checagem do processo `git` -- mesmo padrão da thread de
/// observação de processo do terminal (`WATCH_POLL_INTERVAL` em
/// `porecatu-term/src/terminal.rs`).
const GIT_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Teto de tempo de uma consulta (ADR-0052 §6): estourou, mata o filho e
/// o resultado é falha, que entra em recuo progressivo -- generoso o
/// bastante para uma rede lenta, curto o bastante para não represar a
/// consulta seguinte atrás de uma VPN fora do ar por muito tempo.
const GIT_PROCESS_TIMEOUT: Duration = Duration::from_secs(20);

/// Lê `HEAD` do zero, sem o cache de [`GitInfo`] -- só para o momento em
/// que um resultado chega da thread (ADR-0052 §3): decide se ele ainda
/// pertence à branch atual do repositório, independente de qual janela
/// está mostrando o quê. Não é o caminho quente -- roda só quando uma
/// consulta termina, nunca por frame.
pub(crate) fn current_branch(repo_root: &Path) -> Option<String> {
    let (_, head_path) = find_head_file(repo_root)?;
    let content = std::fs::read_to_string(head_path).ok()?;
    match parse_head(&content)? {
        Head::Branch(name) => Some(name),
        Head::Detached(_) => None,
    }
}

/// ADR-0052 §7: repositório raso não é consultado -- o `fetch` poderia
/// aprofundá-lo, caro e não pedido. Detectar é um `stat` em
/// `.git/shallow`, a mesma classe de custo que a descoberta de
/// repositório já paga.
fn is_shallow_repository(repo_root: &Path) -> bool {
    let Some((_, head_path)) = find_head_file(repo_root) else {
        return false;
    };
    head_path.with_file_name("shallow").exists()
}

/// Resolve o `.git` de `repo_root` (diretório ou arquivo `gitdir:`,
/// worktree/submódulo) e devolve o texto de `config` de lá -- ou do
/// repositório comum, se `commondir` apontar para ele (worktrees não têm
/// `[branch ...]` no próprio `config`, ele mora no principal).
fn read_branch_config(repo_root: &Path) -> Option<String> {
    let dot_git = repo_root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let content = std::fs::read_to_string(&dot_git).ok()?;
        let gitdir = parse_gitdir_file(&content)?;
        if gitdir.is_absolute() {
            gitdir
        } else {
            repo_root.join(gitdir)
        }
    };
    let git_dir = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(content) => {
            let rel = content.lines().next().unwrap_or("").trim();
            if rel.is_empty() {
                git_dir
            } else {
                let common = PathBuf::from(rel);
                if common.is_absolute() {
                    common
                } else {
                    git_dir.join(common)
                }
            }
        }
        Err(_) => git_dir,
    };
    std::fs::read_to_string(git_dir.join("config")).ok()
}

/// Args de `git fetch` (ADR-0052 §6): sem atualizar nada além de
/// `refs/remotes/*` e `FETCH_HEAD` -- não toca árvore, índice nem `HEAD`
/// local (RF-13.4). `--quiet` porque a saída de progresso não interessa
/// a ninguém aqui; o que importa de `stderr` é só o de falha.
fn fetch_args(repo: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-C"),
        repo.as_os_str().to_owned(),
        OsString::from("fetch"),
        OsString::from("--quiet"),
    ]
}

/// Args de `git pull --ff-only` (ADR-0052 §9, RF-13.12): a única forma de
/// integração que este recurso executa. `--ff-only` é a proteção contra
/// árvore suja e branch divergida -- o `git` recusa sem tocar nada nos
/// dois casos, então não há checagem própria a duplicar aqui (RF-13.9 já
/// decide o `clickable` antes do clique existir; isto é o cinto e a
/// suspensório do próprio `git` para o que mudou entre o layout do quadro
/// e o clique).
fn pull_args(repo: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-C"),
        repo.as_os_str().to_owned(),
        OsString::from("pull"),
        OsString::from("--ff-only"),
    ]
}

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    // ADR-0052 §6: a flag é API **segura** da extensão de `Command` para
    // Windows -- verificado ao escrever aquela decisão, com um binário
    // mínimo sob `#![deny(unsafe_code)]`. `unsafe_code = "deny"` do
    // workspace continua sem exceção.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

enum GitProcessResult {
    Exited {
        success: bool,
        stdout: String,
        stderr: String,
    },
    /// `Command::spawn` falhou -- tipicamente `ErrorKind::NotFound`: não
    /// há `git` nenhum para rodar (RF-13.19).
    FailedToStart,
    /// Estourou [`GIT_PROCESS_TIMEOUT`]; o filho já foi morto.
    TimedOut,
}

/// Lança `git` com o vetor de não-interação (ADR-0052 §6) e um teto de
/// tempo por checagem em intervalo curto -- o molde é a thread de
/// observação de processo de `porecatu-term/src/terminal.rs`
/// (`WATCH_POLL_INTERVAL`/`watch_loop`). `Stdio::piped()` para os dois
/// canais, com uma thread de leitura dedicada para cada um -- a mesma
/// razão da thread de leitura de PTY (ADR-0007): sem elas, um `stdout`/
/// `stderr` que enche o buffer do SO travaria o `git` esperando alguém
/// ler enquanto este laço só faz `try_wait`, o deadlock clássico de pipe
/// cheio que `Stdio::null()` evita só quando a saída não importa.
fn spawn_git_command(args: &[OsString], timeout: Duration) -> GitProcessResult {
    let mut command = Command::new("git");
    command.args(args);
    for (key, value) in non_interactive_env() {
        command.env(key, value);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return GitProcessResult::FailedToStart,
    };

    let stdout_handle = child.stdout.take().map(|mut out| {
        thread::spawn(move || {
            let mut buf = String::new();
            let _ = out.read_to_string(&mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|mut err| {
        thread::spawn(move || {
            let mut buf = String::new();
            let _ = err.read_to_string(&mut buf);
            buf
        })
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                thread::sleep(GIT_PROCESS_POLL_INTERVAL);
            }
            Err(_) => break None,
        }
    };

    let stdout = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    match status {
        Some(status) => GitProcessResult::Exited {
            success: status.success(),
            stdout,
            stderr,
        },
        None => GitProcessResult::TimedOut,
    }
}

fn failed(repo: PathBuf, branch: String, failure: QueryFailure) -> RemoteQueryResult {
    RemoteQueryResult {
        repo,
        branch,
        outcome: RemoteQueryOutcome::Failed(failure),
    }
}

/// Executa uma consulta completa: sem upstream ou raso, nem processo
/// nenhum roda (ADR-0052 §7); senão, `fetch` seguido do `rev-list` que
/// traz os dois números (§7). Roda dentro da thread que [`spawn_query`]
/// abre -- síncrona aqui, porque a própria thread já é o mecanismo de
/// "fora do caminho de frame" (ADR-0052 §5).
fn run_query(repo_root: PathBuf, branch: String, timeout: Duration) -> RemoteQueryResult {
    if is_shallow_repository(&repo_root) {
        return RemoteQueryResult {
            repo: repo_root,
            branch,
            outcome: RemoteQueryOutcome::Shallow,
        };
    }
    if let Some(config_text) = read_branch_config(&repo_root)
        && !branch_has_upstream(&config_text, &branch)
    {
        return RemoteQueryResult {
            repo: repo_root,
            branch,
            outcome: RemoteQueryOutcome::NoUpstream,
        };
    }

    match spawn_git_command(&fetch_args(&repo_root), timeout) {
        GitProcessResult::FailedToStart => {
            return failed(repo_root, branch, QueryFailure::GitMissing);
        }
        GitProcessResult::TimedOut => return failed(repo_root, branch, QueryFailure::Network),
        GitProcessResult::Exited {
            success: false,
            stderr,
            ..
        } => return failed(repo_root, branch, classify_failure(false, &stderr)),
        GitProcessResult::Exited { success: true, .. } => {}
    }

    match spawn_git_command(&rev_list_args(&repo_root), timeout) {
        GitProcessResult::FailedToStart => failed(repo_root, branch, QueryFailure::GitMissing),
        GitProcessResult::TimedOut => failed(repo_root, branch, QueryFailure::Network),
        GitProcessResult::Exited {
            success: false,
            stderr,
            ..
        } => failed(repo_root, branch, classify_failure(false, &stderr)),
        GitProcessResult::Exited {
            success: true,
            stdout,
            ..
        } => match parse_rev_list_count(&stdout) {
            Some((behind, ahead)) => RemoteQueryResult {
                repo: repo_root,
                branch,
                outcome: RemoteQueryOutcome::Counted { behind, ahead },
            },
            None => failed(repo_root, branch, QueryFailure::Other),
        },
    }
}

/// Uma consulta ao remoto, numa thread própria e detached (ADR-0052 §5)
/// -- mesma disciplina de [`crate::reload::watch`]: sem `join`, o
/// processo inteiro sai junto dela. `on_result` roda **na thread da
/// consulta**, nunca na main; quem chama passa um fecho que só manda o
/// resultado pelo `EventLoopProxy`.
pub(crate) fn spawn_query(
    repo_root: PathBuf,
    branch: String,
    on_result: impl FnOnce(RemoteQueryResult) + Send + 'static,
) {
    thread::spawn(move || {
        let result = run_query(repo_root, branch, GIT_PROCESS_TIMEOUT);
        on_result(result);
    });
}

/// Executa `pull --ff-only` só. Roda dentro da thread que
/// [`spawn_integration`] abre, pelo mesmo motivo de [`run_query`]: a
/// própria thread já é o "fora do caminho de frame" (ADR-0052 §5).
/// `stdout`/`stderr` viram a mensagem do RF-13.14 quando o `git` recusa --
/// preferindo `stderr` (onde ele de fato escreve o motivo da recusa) e
/// caindo para `stdout` só se o primeiro vier vazio.
fn run_integration(repo_root: PathBuf, timeout: Duration) -> RemoteIntegrationResult {
    match spawn_git_command(&pull_args(&repo_root), timeout) {
        GitProcessResult::FailedToStart => RemoteIntegrationResult {
            repo: repo_root,
            outcome: IntegrationOutcome::Failed {
                message: "o git não foi encontrado no sistema.".to_owned(),
            },
        },
        GitProcessResult::TimedOut => RemoteIntegrationResult {
            repo: repo_root,
            outcome: IntegrationOutcome::Failed {
                message: "o git não respondeu a tempo.".to_owned(),
            },
        },
        GitProcessResult::Exited { success: true, .. } => RemoteIntegrationResult {
            repo: repo_root,
            outcome: IntegrationOutcome::Success,
        },
        GitProcessResult::Exited {
            success: false,
            stdout,
            stderr,
        } => {
            let message = [stderr, stdout]
                .into_iter()
                .map(|s| s.trim().to_owned())
                .find(|s| !s.is_empty())
                .unwrap_or_else(|| "o git recusou a integração sem detalhar o motivo.".to_owned());
            RemoteIntegrationResult {
                repo: repo_root,
                outcome: IntegrationOutcome::Failed { message },
            }
        }
    }
}

/// Integração ao remoto, numa thread própria e detached -- mesma
/// disciplina de [`spawn_query`] (ADR-0052 §5, RF-13.13): a interface não
/// espera o `git` responder, e o resultado chega pelo mesmo tipo de
/// evento, nunca um comando pra mostrar algo.
pub(crate) fn spawn_integration(
    repo_root: PathBuf,
    on_result: impl FnOnce(RemoteIntegrationResult) + Send + 'static,
) {
    thread::spawn(move || {
        let result = run_integration(repo_root, GIT_PROCESS_TIMEOUT);
        on_result(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_branch() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n"),
            Some(Head::Branch("main".to_owned()))
        );
    }

    #[test]
    fn branch_name_keeps_its_own_slashes() {
        // `feat/x` é o nome da branch inteiro. Cortar no último `/`
        // mostraria só `x`, que é outra branch possível no mesmo repo.
        assert_eq!(
            parse_head("ref: refs/heads/feat/porecatu-ui\n"),
            Some(Head::Branch("feat/porecatu-ui".to_owned()))
        );
    }

    #[test]
    fn detached_head_is_the_short_sha() {
        let sha = "80722d9e1f4a6b3c5d8e0f2a4b6c8d0e2f4a6b8c";
        assert_eq!(
            parse_head(sha),
            Some(Head::Detached("80722d9".to_owned())),
            "os mesmos 7 hex que o git usa ao abreviar"
        );
    }

    #[test]
    fn sha256_detached_head_also_works() {
        let sha = "a".repeat(64);
        assert_eq!(parse_head(&sha), Some(Head::Detached("aaaaaaa".to_owned())));
    }

    #[test]
    fn garbage_and_partial_writes_yield_nothing() {
        // Ler o arquivo no meio de uma escrita do git: melhor esconder o
        // segmento por um frame que desenhar lixo (ADR-0049 §2).
        for content in ["", "\n", "ref:", "ref: ", "não é um head", "80722d9"] {
            assert_eq!(parse_head(content), None, "conteúdo {content:?}");
        }
    }

    #[test]
    fn a_sha_of_the_wrong_length_is_not_a_sha() {
        assert_eq!(parse_head(&"a".repeat(39)), None);
        assert_eq!(parse_head(&"a".repeat(41)), None);
        assert_eq!(parse_head(&"z".repeat(40)), None, "não é hex");
    }

    #[test]
    fn gitdir_file_of_a_worktree() {
        assert_eq!(
            parse_gitdir_file("gitdir: /repo/.git/worktrees/wt\n"),
            Some(PathBuf::from("/repo/.git/worktrees/wt"))
        );
        assert_eq!(
            parse_gitdir_file("gitdir: ../.git/modules/sub"),
            Some(PathBuf::from("../.git/modules/sub"))
        );
    }

    #[test]
    fn a_dot_git_file_without_gitdir_is_not_a_repository() {
        for content in ["", "qualquer coisa", "gitdir:", "gitdir:   "] {
            assert_eq!(parse_gitdir_file(content), None, "conteúdo {content:?}");
        }
    }

    #[test]
    fn no_cwd_means_no_work_and_no_branch() {
        let mut info = GitInfo::default();
        assert_eq!(info.head(None), None);
        assert!(
            info.resolved_for.is_none(),
            "sem `cwd` não há nem o que descobrir"
        );
    }

    #[test]
    fn this_very_repository_reports_its_branch() {
        // O teste roda dentro do repositório do Porecatu, então a
        // descoberta tem de achá-lo subindo de `crates/porecatu-ui`.
        let mut info = GitInfo::default();
        let here = std::env::current_dir().expect("cwd do teste");
        let branch = info.head(Some(&here)).map(Head::label);
        assert!(
            branch.is_some(),
            "não achou o repositório subindo de {}",
            here.display()
        );
        assert!(!branch.unwrap().is_empty());
    }

    #[test]
    fn a_directory_outside_any_repository_caches_the_absence() {
        // O `None` também é cacheado: fora de um repositório, a subida de
        // diretórios não pode se repetir a cada frame.
        let mut info = GitInfo::default();
        let root = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        // A raiz do sistema não é um repositório em nenhuma máquina de
        // desenvolvimento sã; se for, o teste não tem o que afirmar.
        if info.head(Some(&root)).is_none() {
            assert_eq!(info.resolved_for.as_deref(), Some(root.as_path()));
            assert!(info.head_path.is_none(), "a ausência ficou registrada");
        }
    }

    #[test]
    fn head_exposes_the_branch_variant_not_just_the_label() {
        // O degrau que a etapa 3 precisa (ADR-0052 §7): quem chama `head`
        // sabe se é uma branch ou um `HEAD` destacado, não só o texto.
        //
        // Fabricado, não a real HEAD do repositório onde o teste roda:
        // `actions/checkout` do GitHub Actions deixa o `.git` do runner em
        // HEAD destacado por padrão (checa um SHA, não uma branch), e
        // `this_very_repository_reports_its_branch` já cobre o caso real
        // -- este teste precisa de uma branch garantida, não da sorte do
        // ambiente. `find_head_file` só olha para `.git/HEAD`, então
        // fabricar os dois arquivos é suficiente, sem `git init`.
        let dir = std::env::temp_dir().join(format!(
            "porecatu-ui-test-branch-head-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".git")).expect("dir de teste");
        std::fs::write(dir.join(".git").join("HEAD"), "ref: refs/heads/main\n")
            .expect("escreve HEAD de teste");
        let mut info = GitInfo::default();
        assert!(matches!(info.head(Some(&dir)), Some(Head::Branch(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn repo_root_is_the_working_tree_ancestor_not_the_dot_git_dir() {
        // PRD-013/ADR-0052 §3: a chave do mapa é o repositório, não o
        // `cwd` -- um `cd` para um subdiretório do mesmo projeto tem de
        // devolver o mesmo `repo_root`.
        let mut info = GitInfo::default();
        let here = std::env::current_dir().expect("cwd do teste");
        let root = info
            .repo_root(Some(&here))
            .expect("repositório encontrado")
            .to_path_buf();
        assert!(!root.join(".git").as_os_str().is_empty());
        assert!(root.join(".git").exists(), "a raiz contém o `.git`");

        let subdir = here.join("src");
        if subdir.is_dir() {
            let root_from_subdir = info.repo_root(Some(&subdir)).map(Path::to_path_buf);
            assert_eq!(
                root_from_subdir,
                Some(root),
                "mesmo repositório, chave estável a um `cd` dentro dele"
            );
        }
    }

    #[test]
    fn repo_root_is_none_outside_any_repository() {
        let mut info = GitInfo::default();
        let root = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        if info.head(Some(&root)).is_none() {
            assert_eq!(info.repo_root(Some(&root)), None);
        }
    }

    // --- PRD-013 / ADR-0052: sincronização com o remoto ---

    #[test]
    fn rev_list_count_parses_the_common_case() {
        assert_eq!(parse_rev_list_count("2\t1\n"), Some((2, 1)));
    }

    #[test]
    fn rev_list_count_survives_crlf() {
        assert_eq!(parse_rev_list_count("3\t0\r\n"), Some((3, 0)));
    }

    #[test]
    fn rev_list_count_accepts_plain_spaces_too() {
        assert_eq!(parse_rev_list_count("0 5\n"), Some((0, 5)));
    }

    #[test]
    fn rev_list_count_of_empty_output_is_none() {
        assert_eq!(parse_rev_list_count(""), None);
    }

    #[test]
    fn rev_list_count_of_garbage_is_none() {
        for output in ["não é git", "abc\tdef\n", "\n\n"] {
            assert_eq!(parse_rev_list_count(output), None, "saída {output:?}");
        }
    }

    #[test]
    fn rev_list_count_with_a_missing_field_is_none() {
        assert_eq!(parse_rev_list_count("2\n"), None);
        assert_eq!(parse_rev_list_count("2\t\n"), None);
    }

    #[test]
    fn rev_list_count_with_an_extra_field_is_none() {
        // Saída que não é a esperada -- melhor não afirmar nada que
        // inventar um par a partir dos dois primeiros números.
        assert_eq!(parse_rev_list_count("2\t1\t9\n"), None);
    }

    #[test]
    fn zero_seconds_turns_the_feature_off() {
        let effective = effective_poll_interval(0);
        assert_eq!(effective.interval, None);
        assert!(!effective.raised_to_floor);
    }

    #[test]
    fn a_value_below_the_floor_is_raised_and_flagged() {
        let effective = effective_poll_interval(1);
        assert_eq!(effective.interval, Some(Duration::from_secs(30)));
        assert!(effective.raised_to_floor);
    }

    #[test]
    fn the_floor_itself_is_not_flagged_as_raised() {
        let effective = effective_poll_interval(30);
        assert_eq!(effective.interval, Some(Duration::from_secs(30)));
        assert!(!effective.raised_to_floor);
    }

    #[test]
    fn an_absurdly_large_value_passes_through_untouched() {
        let effective = effective_poll_interval(u64::MAX);
        assert_eq!(effective.interval, Some(Duration::from_secs(u64::MAX)));
        assert!(!effective.raised_to_floor);
    }

    #[test]
    fn an_in_flight_query_never_redispatches() {
        let now = Instant::now();
        assert!(!is_time_to_query(
            QueryState::InFlight,
            Some(now - Duration::from_secs(10_000)),
            now,
            Duration::from_secs(30)
        ));
    }

    #[test]
    fn no_upstream_never_dispatches() {
        let now = Instant::now();
        assert!(!is_time_to_query(
            QueryState::NoUpstream,
            None,
            now,
            Duration::from_secs(30)
        ));
    }

    #[test]
    fn a_shallow_repository_never_dispatches() {
        let now = Instant::now();
        assert!(!is_time_to_query(
            QueryState::Shallow,
            None,
            now,
            Duration::from_secs(30)
        ));
    }

    #[test]
    fn idle_with_no_previous_query_dispatches_immediately() {
        let now = Instant::now();
        assert!(is_time_to_query(
            QueryState::Idle,
            None,
            now,
            Duration::from_secs(300)
        ));
    }

    #[test]
    fn idle_before_the_interval_elapsed_waits() {
        let now = Instant::now();
        let last = now - Duration::from_secs(100);
        assert!(!is_time_to_query(
            QueryState::Idle,
            Some(last),
            now,
            Duration::from_secs(300)
        ));
    }

    #[test]
    fn idle_after_the_interval_elapsed_dispatches() {
        let now = Instant::now();
        let last = now - Duration::from_secs(301);
        assert!(is_time_to_query(
            QueryState::Idle,
            Some(last),
            now,
            Duration::from_secs(300)
        ));
    }

    #[test]
    fn a_failed_query_backs_off_beyond_the_plain_interval() {
        let now = Instant::now();
        let last = now - Duration::from_secs(301);
        let interval = Duration::from_secs(300);
        // Depois de uma falha, o mesmo tempo que já dispararia em `Idle`
        // ainda não é suficiente: o recuo dobrou o intervalo.
        assert!(!is_time_to_query(
            QueryState::Failed { attempt: 1 },
            Some(last),
            now,
            interval
        ));
        let now = last + interval * 2;
        assert!(is_time_to_query(
            QueryState::Failed { attempt: 1 },
            Some(last),
            now,
            interval
        ));
    }

    #[test]
    fn backoff_caps_instead_of_growing_forever() {
        let interval = Duration::from_secs(30);
        let capped = backoff_interval(interval, 3);
        assert_eq!(capped, interval * 8); // 2^3 == o teto documentado
        // Uma tentativa absurdamente alta não estoura nem regride: o teto
        // continua o mesmo.
        assert_eq!(backoff_interval(interval, 1_000), capped);
    }

    #[test]
    fn zero_interval_schedules_nothing_even_with_entries_pending() {
        // ADR-0052 §1 item 2, RF-13.2: com o recurso desligado, nenhum
        // prazo entra na conta -- nem com repositórios já no mapa de uma
        // execução anterior à mudança de config.
        let now = Instant::now();
        let entries = [RemoteEntry {
            state: QueryState::Idle,
            last_queried_at: Some(now - Duration::from_secs(10_000)),
            ..Default::default()
        }];
        assert_eq!(next_query_deadline(None, entries.iter(), now), None);
    }

    #[test]
    fn no_entries_means_no_deadline() {
        let now = Instant::now();
        let entries: Vec<RemoteEntry> = Vec::new();
        assert_eq!(
            next_query_deadline(Some(Duration::from_secs(300)), entries.iter(), now),
            None
        );
    }

    #[test]
    fn idle_entry_deadline_is_last_queried_plus_interval() {
        let now = Instant::now();
        let last = now - Duration::from_secs(100);
        let entries = [RemoteEntry {
            state: QueryState::Idle,
            last_queried_at: Some(last),
            ..Default::default()
        }];
        let interval = Duration::from_secs(300);
        assert_eq!(
            next_query_deadline(Some(interval), entries.iter(), now),
            Some(last + interval)
        );
    }

    #[test]
    fn failed_entry_deadline_uses_the_backed_off_interval() {
        let now = Instant::now();
        let last = now - Duration::from_secs(100);
        let interval = Duration::from_secs(30);
        let entries = [RemoteEntry {
            state: QueryState::Failed { attempt: 2 },
            last_queried_at: Some(last),
            ..Default::default()
        }];
        assert_eq!(
            next_query_deadline(Some(interval), entries.iter(), now),
            Some(last + backoff_interval(interval, 2))
        );
    }

    #[test]
    fn in_flight_no_upstream_and_shallow_never_contribute_a_deadline() {
        let now = Instant::now();
        let interval = Duration::from_secs(30);
        for state in [
            QueryState::InFlight,
            QueryState::NoUpstream,
            QueryState::Shallow,
        ] {
            let entries = [RemoteEntry {
                state,
                last_queried_at: Some(now - Duration::from_secs(10_000)),
                ..Default::default()
            }];
            assert_eq!(
                next_query_deadline(Some(interval), entries.iter(), now),
                None,
                "{state:?} não deveria agendar nada"
            );
        }
    }

    #[test]
    fn the_earliest_of_several_repositories_wins() {
        let now = Instant::now();
        let interval = Duration::from_secs(300);
        let entries = [
            RemoteEntry {
                state: QueryState::Idle,
                last_queried_at: Some(now - Duration::from_secs(50)),
                ..Default::default()
            },
            RemoteEntry {
                state: QueryState::Idle,
                last_queried_at: Some(now - Duration::from_secs(250)),
                ..Default::default()
            },
        ];
        assert_eq!(
            next_query_deadline(Some(interval), entries.iter(), now),
            Some(now - Duration::from_secs(250) + interval)
        );
    }

    #[test]
    fn a_result_for_the_current_branch_is_stored() {
        assert_eq!(
            incoming_result_decision("main", Some("main")),
            IncomingResultDecision::Store
        );
    }

    #[test]
    fn a_result_is_stored_even_when_its_repository_is_no_longer_the_active_tab() {
        // O mapa é por repositório, não por aba -- a decisão não depende de
        // quem está sendo exibido, só da branch (ADR-0052 §3).
        assert_eq!(
            incoming_result_decision("main", Some("main")),
            IncomingResultDecision::Store
        );
    }

    #[test]
    fn a_result_for_a_branch_that_already_changed_is_discarded() {
        assert_eq!(
            incoming_result_decision("main", Some("feat/x")),
            IncomingResultDecision::Discard
        );
    }

    #[test]
    fn a_result_when_the_repository_has_no_readable_head_anymore_is_discarded() {
        assert_eq!(
            incoming_result_decision("main", None),
            IncomingResultDecision::Discard
        );
    }

    #[test]
    fn no_new_commits_shows_nothing() {
        assert_eq!(ahead_behind_label(0, 0), None);
    }

    #[test]
    fn ahead_only_with_nothing_behind_shows_nothing() {
        // RF-13.8: o segmento existe por causa do que falta buscar, não do
        // que já está pronto pra empurrar.
        assert_eq!(ahead_behind_label(0, 4), None);
    }

    #[test]
    fn one_commit_behind_is_singular() {
        assert_eq!(ahead_behind_label(1, 0), Some("1 commit atrás".to_owned()));
    }

    #[test]
    fn three_commits_behind_is_plural() {
        assert_eq!(ahead_behind_label(3, 0), Some("3 commits atrás".to_owned()));
    }

    #[test]
    fn diverged_branch_shows_both_numbers_without_the_word_commits() {
        assert_eq!(
            ahead_behind_label(2, 1),
            Some("2 atrás, 1 à frente".to_owned())
        );
    }

    #[test]
    fn nothing_ahead_can_integrate() {
        assert!(can_integrate(0));
    }

    #[test]
    fn something_ahead_cannot_integrate() {
        assert!(!can_integrate(1));
    }

    #[test]
    fn rev_list_args_carry_the_repository_path_as_its_own_argument() {
        // O caminho vem intacto, como UM elemento do vetor -- é o que prova
        // que não há concatenação de string de comando (ADR-0052 §6).
        let repo = Path::new("C:/Projetos com espaço/repo");
        let args = rev_list_args(repo);
        assert_eq!(
            args,
            vec![
                OsString::from("-C"),
                OsString::from("C:/Projetos com espaço/repo"),
                OsString::from("rev-list"),
                OsString::from("--count"),
                OsString::from("--left-right"),
                OsString::from("@{u}...HEAD"),
            ]
        );
    }

    #[test]
    fn rev_list_args_never_interpolate_the_path_into_another_argument() {
        let repo = Path::new("/tmp/repo; rm -rf /");
        let args = rev_list_args(repo);
        // Cada argumento é o que é, sem ninguém injetando texto de outro --
        // em particular, nenhum argumento além do próprio caminho o contém.
        for (i, arg) in args.iter().enumerate() {
            if i != 1 {
                assert!(!arg.to_string_lossy().contains("rm -rf"));
            }
        }
        assert_eq!(args[1], OsString::from("/tmp/repo; rm -rf /"));
    }

    #[test]
    fn pull_args_carry_the_repository_path_as_its_own_argument() {
        let repo = Path::new("C:/Projetos com espaço/repo");
        let args = pull_args(repo);
        assert_eq!(
            args,
            vec![
                OsString::from("-C"),
                OsString::from("C:/Projetos com espaço/repo"),
                OsString::from("pull"),
                OsString::from("--ff-only"),
            ]
        );
    }

    #[test]
    fn pull_args_never_interpolate_the_path_into_another_argument() {
        let repo = Path::new("/tmp/repo; rm -rf /");
        let args = pull_args(repo);
        for (i, arg) in args.iter().enumerate() {
            if i != 1 {
                assert!(!arg.to_string_lossy().contains("rm -rf"));
            }
        }
        assert_eq!(args[1], OsString::from("/tmp/repo; rm -rf /"));
    }

    #[test]
    fn pull_args_always_carry_ff_only() {
        // RF-13.12: a única forma de integração que este recurso executa.
        let args = pull_args(Path::new("/repo"));
        assert!(args.iter().any(|a| a == "--ff-only"));
    }

    #[test]
    fn the_non_interactive_vector_covers_all_four_channels() {
        let env = non_interactive_env();
        let keys: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        for expected in [
            "GIT_TERMINAL_PROMPT",
            "GIT_ASKPASS",
            "GIT_SSH_COMMAND",
            "GCM_INTERACTIVE",
        ] {
            assert!(keys.contains(&expected), "canal ausente: {expected}");
        }
        assert_eq!(
            keys.len(),
            4,
            "os quatro canais, nem a mais nem a menos: {keys:?}"
        );
    }

    #[test]
    fn git_missing_is_classified_regardless_of_stderr() {
        assert_eq!(classify_failure(true, ""), QueryFailure::GitMissing);
    }

    #[test]
    fn network_failures_are_classified_as_network() {
        for stderr in [
            "fatal: unable to access 'https://example.com/': Could not resolve host: example.com",
            "ssh: connect to host example.com port 22: Connection timed out",
        ] {
            assert_eq!(classify_failure(false, stderr), QueryFailure::Network);
        }
    }

    #[test]
    fn auth_failures_are_classified_as_auth() {
        for stderr in [
            "remote: Support for password authentication was removed. fatal: Authentication failed",
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled",
        ] {
            assert_eq!(classify_failure(false, stderr), QueryFailure::Auth);
        }
    }

    #[test]
    fn unrecognized_failures_fall_back_to_other() {
        assert_eq!(
            classify_failure(false, "fatal: something we don't recognize"),
            QueryFailure::Other
        );
    }

    #[test]
    fn branch_with_remote_and_merge_has_upstream() {
        let config = "[core]\n\tbare = false\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n";
        assert!(branch_has_upstream(config, "main"));
    }

    #[test]
    fn branch_without_a_section_has_no_upstream() {
        let config = "[core]\n\tbare = false\n";
        assert!(!branch_has_upstream(config, "main"));
    }

    #[test]
    fn a_different_branchs_section_does_not_count() {
        // Só a seção da branch pedida importa -- uma branch nova, nunca
        // empurrada, não deve "herdar" o upstream de outra.
        let config = "[branch \"other\"]\n\tremote = origin\n\tmerge = refs/heads/other\n";
        assert!(!branch_has_upstream(config, "main"));
    }

    #[test]
    fn remote_without_merge_is_not_a_complete_upstream() {
        let config = "[branch \"main\"]\n\tremote = origin\n";
        assert!(!branch_has_upstream(config, "main"));
    }

    #[test]
    fn branch_name_with_slash_matches_its_own_quoted_subsection() {
        let config = "[branch \"feat/x\"]\n\tremote = origin\n\tmerge = refs/heads/feat/x\n";
        assert!(branch_has_upstream(config, "feat/x"));
        assert!(!branch_has_upstream(config, "feat/y"));
    }

    #[test]
    fn section_name_is_case_insensitive_but_branch_name_is_not() {
        let config = "[Branch \"Main\"]\n\tremote = origin\n\tmerge = refs/heads/Main\n";
        assert!(branch_has_upstream(config, "Main"));
        assert!(
            !branch_has_upstream(config, "main"),
            "nome de branch é sensível a maiúsculas"
        );
    }
}
