// SPDX-License-Identifier: GPL-3.0-or-later

//! Encerramento robusto de árvore de processos (ADR-0033, ADR-0035-bis --
//! ver a nota abaixo sobre a varredura complementar). No Windows,
//! [`ProcessGroup`] combina duas técnicas:
//!
//! 1. Um **Job Object** com `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: fechar o
//!    último handle do Job mata instantaneamente todo processo que
//!    **propagou** a associação ao Job -- sem varrer nada, o kernel já
//!    mantém essa lista. É o caminho rápido, e cobre `cmd.exe` e o
//!    Windows PowerShell (5.1) inteiros.
//! 2. Uma **varredura da árvore de processos do SO** (`sysinfo`), a partir
//!    do PID do processo raiz, usada como complemento -- **não
//!    substituto** -- do Job. Achado na investigação do relato do
//!    usuário: com **PowerShell 7 (pwsh)** como shell, os descendentes
//!    (`cmd.exe`/`node.exe` de um `npm start`, por exemplo) **nunca**
//!    entram no Job, mesmo com a árvore de processos do SO
//!    (`ParentProcessId`) inteiramente correta -- confirmado com
//!    `Get-CimInstance Win32_Process`, subindo a cadeia até o `pwsh.exe`
//!    raiz sem um único elo quebrado. A causa exata a nível de syscall
//!    não foi fechada (precisaria de rastreamento de API tipo Process
//!    Monitor); o fato empírico, reproduzido, é que o Job sozinho
//!    sub-conta e sub-mata nesse shell -- que é o **default preferido**
//!    do Porecatu quando instalado (`resolve_default_shell`).
//!
//! A varredura roda **uma vez**, disparada só pelo fechamento pedido pelo
//! usuário ou pela consulta de contagem (ADR-0034) -- nunca continuamente,
//! e nunca para decidir "qual processo é o de primeiro plano" (isso
//! continua sendo `TermModes`, ADR-0017). Alvo (`root_pid`) e gatilho já
//! são conhecidos sem ambiguidade; isto não é a varredura que o
//! ADR-0005/ADR-0008/ADR-0017 rejeitaram para fins de *detecção*.
//!
//! Fora do Windows, `for_child` sempre devolve `None` -- dívida assumida,
//! sem ambiente Unix disponível pra implementar `setsid`+`killpg` (mesmo
//! padrão de outras verificações interativas registradas no CLAUDE.md).
//!
//! **Não mora dentro de `PtyHandle`.** `PtyHandle` é vazado de propósito
//! na saída natural do shell (`leak_pty`, em `porecatu-term`) pra não
//! repetir o deadlock do `ClosePseudoConsole` -- se o Job estivesse
//! guardado ali dentro, seria esquecido junto nesse `mem::forget`, e o
//! `KILL_ON_JOB_CLOSE` nunca dispararia: o mesmo bug que este módulo
//! existe pra corrigir, só que reimplementado. Por isso [`spawn`](crate::spawn)
//! devolve `ProcessGroup` **separado** do `PtyHandle`: quem chama decide,
//! por caminho de saída, se mata a árvore (`kill_tree`, no fechamento
//! pedido pelo usuário) ou esquece (`mem::forget`, preservando um
//! processo que o shell tenha deliberadamente destacado, ex. `start /b
//! algo & exit`).

use portable_pty::Child;
#[cfg(windows)]
use std::sync::Arc;

/// Conjunto de processos descendentes de um shell spawnado. `Clone` barato
/// (`root_pid` é `Copy`, o Job é um `Arc`) -- é assim que a mesma
/// referência chega tanto ao lado da UI (consulta de contagem, ADR-0034)
/// quanto à thread de observação do PTY (kill, ADR-0033) sem que nenhum
/// dos dois seja "dono único" do Job.
#[derive(Debug, Clone)]
pub struct ProcessGroup {
    #[cfg_attr(not(windows), allow(dead_code))]
    root_pid: u32,
    #[cfg(windows)]
    job: Option<Arc<win32job::Job>>,
}

impl ProcessGroup {
    /// `root_pid` vem de `child.process_id()` -- sem ele não há como
    /// varrer descendentes nem contar, então é a única falha que impede
    /// `ProcessGroup` de existir. O Job em si é best-effort: falha em
    /// criá-lo ou atribuí-lo (`create_job_for`) degrada para `job: None`,
    /// sem impedir a construção -- a varredura por `sysinfo` continua
    /// funcionando (mais lenta que o Job, mas funcional) mesmo sem ele.
    #[cfg(windows)]
    pub fn for_child(child: &dyn Child) -> Option<Self> {
        let root_pid = child.process_id()?;
        let job = Self::create_job_for(child);
        Some(Self { root_pid, job })
    }

    #[cfg(windows)]
    fn create_job_for(child: &dyn Child) -> Option<Arc<win32job::Job>> {
        let mut info = win32job::ExtendedLimitInfo::new();
        info.limit_kill_on_job_close();
        let job = match win32job::Job::create_with_limit_info(&info) {
            Ok(job) => job,
            Err(err) => {
                eprintln!("porecatu: Job Object não criado, killtree só por varredura: {err}");
                return None;
            }
        };
        let Some(handle) = child.as_raw_handle() else {
            eprintln!("porecatu: handle do processo ausente, killtree só por varredura");
            return None;
        };
        if let Err(err) = job.assign_process(handle as isize) {
            eprintln!("porecatu: processo não atribuído ao Job, killtree só por varredura: {err}");
            return None;
        }
        Some(Arc::new(job))
    }

    #[cfg(not(windows))]
    pub fn for_child(_child: &dyn Child) -> Option<Self> {
        None
    }

    /// Quantos processos estão vivos na árvore agora -- inclui o shell
    /// raiz. **União** de duas fontes, porque cada uma tem um ponto cego
    /// que a outra cobre:
    ///
    /// - A lista do Job (`query_process_id_list`) inclui todo processo
    ///   que alguma vez propagou a associação, **mesmo que o pai
    ///   intermediário já tenha morrido** -- é como um `cmd /c start /b
    ///   algo` sobrevive sozinho depois de o `cmd.exe` que o lançou sair
    ///   (membresia de Job é permanente, não depende do pai continuar
    ///   vivo). Mas não vê nada que nunca entrou no Job (pwsh).
    /// - A varredura por `sysinfo` (`descendants_of`) vê qualquer
    ///   descendente **cuja cadeia de ancestrais até `root_pid` esteja
    ///   inteira viva agora** -- é o que cobre pwsh, mas não alcança um
    ///   descendente cujo pai direto já morreu (o link fica quebrado:
    ///   processo morto não aparece em `system.processes()` pra ligar o
    ///   neto ao avô).
    ///
    /// Juntas cobrem os dois casos reais: shell que propaga corretamente
    /// (cmd.exe, PowerShell 5.1) e shell que não propaga (pwsh 7) --
    /// desde que, neste último caso, a cadeia esteja viva no momento da
    /// consulta (é o que acontece com um `npm start`/servidor de longa
    /// duração em primeiro plano: shell, `cmd.exe` do `npm.cmd`, `node`
    /// do `npm-cli.js` e o `node` do servidor ficam vivos juntos).
    #[cfg(windows)]
    pub fn process_count(&self) -> usize {
        self.live_pids().len()
    }

    #[cfg(windows)]
    fn live_pids(&self) -> std::collections::HashSet<u32> {
        let mut pids = std::collections::HashSet::new();
        pids.insert(self.root_pid);
        if let Some(job) = &self.job
            && let Ok(list) = job.query_process_id_list()
        {
            pids.extend(list.into_iter().map(|pid| pid as u32));
        }
        let system = refreshed_system();
        for pid in descendants_of(&system, sysinfo::Pid::from_u32(self.root_pid)) {
            pids.insert(pid.as_u32());
        }
        pids
    }

    #[cfg(not(windows))]
    pub fn process_count(&self) -> usize {
        1
    }

    /// Mata a árvore inteira: fecha o handle do Job primeiro (mata na
    /// hora quem propagou a associação -- inclusive quem sobreviveu ao
    /// próprio pai intermediário, ex. `start /b`) e, na sequência, mata
    /// qualquer descendente ainda vivo que a varredura por `sysinfo`
    /// encontrar (cobre pwsh, cuja cadeia costuma estar inteira viva no
    /// momento do fechamento -- ver `process_count`).
    #[cfg(windows)]
    pub fn kill_tree(self) {
        drop(self.job);
        let system = refreshed_system();
        for pid in descendants_of(&system, sysinfo::Pid::from_u32(self.root_pid)) {
            if let Some(process) = system.process(pid) {
                let _ = process.kill();
            }
        }
    }

    #[cfg(not(windows))]
    pub fn kill_tree(self) {}
}

#[cfg(windows)]
fn refreshed_system() -> sysinfo::System {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system
}

/// Todos os descendentes de `root` (não inclui `root`), varrendo a lista
/// de processos do SO **uma vez** (`system` já veio atualizado) --
/// `O(processos do sistema)`, aceitável porque só roda no fechamento de
/// aba ou na consulta de contagem, nunca em loop.
#[cfg(windows)]
fn descendants_of(system: &sysinfo::System, root: sysinfo::Pid) -> Vec<sysinfo::Pid> {
    let mut children_of: std::collections::HashMap<sysinfo::Pid, Vec<sysinfo::Pid>> =
        std::collections::HashMap::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            children_of.entry(parent).or_default().push(*pid);
        }
    }
    let mut result = Vec::new();
    let mut frontier = vec![root];
    while let Some(pid) = frontier.pop() {
        if let Some(children) = children_of.get(&pid) {
            for &child in children {
                result.push(child);
                frontier.push(child);
            }
        }
    }
    result
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::spawn::{PtySize, SpawnConfig};

    fn size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// Sobe `program` interativo (sem `/c`/`-Command` -- igual ao spawn
    /// real de uma aba), responde ao DSR de posição de cursor que o
    /// ConPTY manda no handshake (sem isso o shell fica preso esperando
    /// uma resposta que nunca chega, mesma nota de `tests/spawn.rs`), e
    /// escreve `command` assim que `prompt_marker` aparecer na saída --
    /// reage dentro do mesmo ciclo de leitura, sem precisar de um segundo
    /// `writer` (só dá pra tomar um, `take_writer` é de uso único).
    fn spawn_interactive_and_run(
        program: &str,
        cwd: Option<std::path::PathBuf>,
        prompt_marker: &'static str,
        command: &'static str,
    ) -> (crate::spawn::PtyHandle, ProcessGroup) {
        use std::io::{Read, Write};

        let config = SpawnConfig {
            program: Some(program.to_string()),
            args: Vec::new(),
            env: Vec::new(),
            cwd,
            size: size(),
        };
        let (handle, group) = crate::spawn::spawn(config).expect("spawn deve funcionar");
        let group = group.expect("ProcessGroup deve ser criado em Windows");

        let mut reader = handle.reader().expect("reader");
        let mut writer = handle.writer().expect("writer");
        const DSR: &[u8] = b"\x1b[6n";
        const DSR_REPLY: &[u8] = b"\x1b[1;1R";
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut all = Vec::new();
            let mut answered_dsr = false;
            let mut sent_command = false;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        all.extend_from_slice(&buf[..n]);
                        if !answered_dsr && all.windows(DSR.len()).any(|w| w == DSR) {
                            let _ = writer.write_all(DSR_REPLY);
                            let _ = writer.flush();
                            answered_dsr = true;
                        }
                        if !sent_command && String::from_utf8_lossy(&all).contains(prompt_marker) {
                            let _ = writer.write_all(command.as_bytes());
                            let _ = writer.write_all(b"\r\n");
                            let _ = writer.flush();
                            sent_command = true;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        (handle, group)
    }

    /// Sobe um shell interativo real (sem UI/janela/foco -- só processo)
    /// e roda um comando em primeiro plano (`ping`, no lugar de um `npm
    /// start`/servidor de longa duração) que fica vivo enquanto o shell
    /// espera -- exatamente o padrão real de uma aba do Porecatu, ao
    /// contrário de um `cmd /c start /b` que desanexa e mata a cadeia
    /// junto com o `/c` que sai na hora (achado desta investigação: isso
    /// não é o cenário real, e o teardown do console do ConPTY pode matar
    /// o processo desanexado antes mesmo de medir). Confirma que
    /// `kill_tree` mata **os dois** -- shell e comando -- e que não
    /// bloqueia por mais que um instante (a evidência empírica de "não é
    /// `ClosePseudoConsole`" que a investigação da F4 pediu).
    #[test]
    fn kill_tree_kills_the_whole_tree_via_job() {
        // `cmd.exe` explícito, não `resolve_default_shell` -- este teste
        // valida a mecânica do killtree com um shell que propaga
        // corretamente a associação ao Job (cmd.exe e Windows PowerShell
        // 5.1 propagam; PowerShell 7/pwsh não -- ver o teste seguinte e a
        // nota do módulo).
        let (mut handle, group) =
            spawn_interactive_and_run("cmd.exe", None, ">", "ping -n 30 127.0.0.1");

        // Espera o `ping` subir como filho direto do `cmd.exe` antes de
        // medir a contagem.
        std::thread::sleep(std::time::Duration::from_millis(800));
        let count_before = group.process_count();
        assert!(
            count_before >= 2,
            "esperava shell + ping, contou {count_before}"
        );

        let start = std::time::Instant::now();
        group.kill_tree();
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "kill_tree não pode bloquear como o ClosePseudoConsole -- levou {elapsed:?}"
        );

        std::thread::sleep(std::time::Duration::from_millis(300));
        let exited = handle.try_wait().ok().flatten().is_some();
        assert!(exited, "shell raiz deveria ter morrido");
    }

    /// A reprodução real do relato do usuário: PowerShell 7 (`pwsh.exe`)
    /// interativo rodando um comando de longa duração em primeiro plano
    /// (`ping`, aqui no lugar de `npm start`) -- exatamente como pwsh fica
    /// bloqueado esperando um `npm start` interativo. Confirma que a
    /// varredura por `sysinfo` mata o processo mesmo quando ele nunca
    /// entrou no Job -- sem isso, este teste falharia exatamente como o
    /// app falhava (o achado da investigação: subindo a cadeia via
    /// `Get-CimInstance Win32_Process` a partir do `node.exe` de um `npm
    /// start`, cada elo de pai-filho até o `pwsh.exe` raiz estava
    /// correto, mas nenhum desses processos aparecia na lista do Job).
    #[test]
    fn kill_tree_kills_descendants_that_never_joined_the_job() {
        // Requer `pwsh` instalado -- se não estiver, pula (mesmo padrão
        // de degradação do resto do módulo: ambiente sem a ferramenta não
        // é uma falha do código).
        if !crate::shell::search_path("pwsh.exe") {
            eprintln!("pwsh.exe não encontrado, pulando teste");
            return;
        }
        let (mut handle, group) =
            spawn_interactive_and_run("pwsh.exe", None, "PS ", "ping -n 30 127.0.0.1");

        std::thread::sleep(std::time::Duration::from_millis(1500));
        let root_pid = group.root_pid;
        let system_before = refreshed_system();
        let descendants_before: Vec<sysinfo::Pid> =
            descendants_of(&system_before, sysinfo::Pid::from_u32(root_pid));
        assert!(
            !descendants_before.is_empty(),
            "esperava o `ping` vivo como descendente antes de matar"
        );
        // A prova do achado: nenhum desses descendentes está na lista do
        // Job, apesar de estarem vivos e corretamente ligados ao PID raiz.
        let job_pids: Vec<u32> = group
            .job
            .as_ref()
            .and_then(|j| j.query_process_id_list().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|pid| pid as u32)
            .collect();
        assert!(
            descendants_before
                .iter()
                .any(|pid| !job_pids.contains(&pid.as_u32())),
            "esperava achar um descendente vivo que o Job não vê -- \
             se isto falhar, talvez o pwsh tenha passado a propagar a \
             associação (o que seria ótimo, mas revisar a nota do módulo)"
        );

        group.kill_tree();
        std::thread::sleep(std::time::Duration::from_millis(300));

        let system_after = refreshed_system();
        for pid in &descendants_before {
            assert!(
                system_after.process(*pid).is_none(),
                "pid {pid} deveria ter morrido com kill_tree, mas sobreviveu"
            );
        }

        let exited = handle.try_wait().ok().flatten().is_some();
        assert!(exited, "pwsh raiz deveria ter morrido");
    }

    /// Falha de atribuição não deve entrar em pânico -- degrada para
    /// `job: None`, e `ProcessGroup` continua existindo (a varredura por
    /// `sysinfo` não depende do Job).
    #[test]
    fn for_child_degrades_gracefully_on_invalid_handle() {
        struct FakeChild;
        impl std::fmt::Debug for FakeChild {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "FakeChild")
            }
        }
        impl portable_pty::ChildKiller for FakeChild {
            fn kill(&mut self) -> std::io::Result<()> {
                Ok(())
            }
            fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
                Box::new(FakeChild)
            }
        }
        impl portable_pty::Child for FakeChild {
            fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
                Ok(None)
            }
            fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
                unreachable!("não chamado neste teste")
            }
            fn process_id(&self) -> Option<u32> {
                Some(std::process::id())
            }
            fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
                None
            }
        }

        let group = ProcessGroup::for_child(&FakeChild).expect("root_pid presente");
        assert!(group.job.is_none(), "sem handle, o Job não deveria existir");
        // A contagem ainda funciona via sysinfo, usando o PID do processo
        // de teste em si como raiz -- conta ao menos ele mesmo. Não
        // assume exatamente 1: outros testes deste binário rodam em
        // paralelo por padrão (`cargo test`) e podem ter spawnado shells
        // reais como filhos do processo de teste no instante da medição.
        assert!(group.process_count() >= 1);
    }

    /// Sem `process_id()`, não há `root_pid` -- `ProcessGroup` não pode
    /// existir de jeito nenhum (nem Job, nem varredura têm o que rastrear).
    #[test]
    fn for_child_without_pid_returns_none() {
        struct NoPidChild;
        impl std::fmt::Debug for NoPidChild {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "NoPidChild")
            }
        }
        impl portable_pty::ChildKiller for NoPidChild {
            fn kill(&mut self) -> std::io::Result<()> {
                Ok(())
            }
            fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
                Box::new(NoPidChild)
            }
        }
        impl portable_pty::Child for NoPidChild {
            fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
                Ok(None)
            }
            fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
                unreachable!("não chamado neste teste")
            }
            fn process_id(&self) -> Option<u32> {
                None
            }
            fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
                None
            }
        }

        assert!(ProcessGroup::for_child(&NoPidChild).is_none());
    }
}
