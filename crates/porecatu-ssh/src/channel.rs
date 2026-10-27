// SPDX-License-Identifier: GPL-3.0-or-later

//! A API síncrona de um canal SSH (ADR-0072 §4). Por baixo, uma tarefa do
//! runtime (`crate::task`) fala com o servidor; daqui só se vê `std`.

use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::ask::Asker;
use crate::end::{End, FailKind};
use crate::known_hosts::KnownHostsFiles;
use crate::runtime;
use crate::spawn::{Connection, ConnectionId, OpenError, SshSpawn};
use crate::task;
use crate::vault::{SystemVault, Vault};

/// O que a tarefa recebe do lado síncrono. Escrita e controle viajam no mesmo
/// canal, para a tarefa esperar por uma coisa só.
pub(crate) enum Cmd {
    Data(Vec<u8>),
    Resize { rows: u16, cols: u16 },
    Close,
}

/// Onde o fim de um canal é guardado. O primeiro vale; o resto é ignorado.
#[derive(Clone, Default)]
pub(crate) struct EndSlot(Arc<Mutex<Option<End>>>);

impl EndSlot {
    pub(crate) fn set(&self, end: End) {
        let mut slot = self.lock();
        if slot.is_none() {
            *slot = Some(end);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<End>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Consulta, de qualquer thread, se o canal ainda está aberto ou abrindo.
#[derive(Clone)]
pub struct EndProbe(EndSlot);

impl EndProbe {
    /// `true` até o canal terminar (ou falhar ao abrir).
    pub fn is_open(&self) -> bool {
        self.0.lock().is_none()
    }
}

/// Um canal SSH com pseudo-terminal.
pub struct SshChannel {
    reader: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    cmd: UnboundedSender<Cmd>,
    end: EndSlot,
    connection: ConnectionId,
}

impl SshChannel {
    /// Abre um canal e **devolve na hora**: conectar, autenticar e abrir o
    /// canal acontecem no runtime. O resultado aparece em [`try_end`] e, para
    /// o que chegar do servidor, em [`reader`].
    ///
    /// A chave do host é conferida contra o `known_hosts` do OpenSSH
    /// (ADR-0074): host conhecido segue; desconhecido vira uma pergunta ao
    /// `asker`, e só `Confirm` conecta (e grava); alterada ou revogada encerra.
    ///
    /// [`try_end`]: SshChannel::try_end
    /// [`reader`]: SshChannel::reader
    pub fn open(spawn: SshSpawn, asker: Arc<dyn Asker>) -> Result<Self, OpenError> {
        Self::open_with(
            spawn,
            asker,
            KnownHostsFiles::standard(),
            Arc::new(SystemVault),
        )
    }

    /// Como [`open`](SshChannel::open), com os arquivos de `known_hosts` e o
    /// cofre à escolha -- o que os testes usam para não tocar o `~/.ssh` nem o
    /// cofre de quem roda.
    pub(crate) fn open_with(
        spawn: SshSpawn,
        asker: Arc<dyn Asker>,
        files: KnownHostsFiles,
        vault: Arc<dyn Vault>,
    ) -> Result<Self, OpenError> {
        if matches!(spawn.connection, Connection::Share(_)) {
            return Err(OpenError::ShareUnsupported);
        }
        let (data_tx, data_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = unbounded_channel();
        let end = EndSlot::default();
        let connection = ConnectionId::next();

        let started = runtime::spawn(task::run(
            spawn,
            files,
            asker,
            vault,
            data_tx,
            cmd_tx.clone(),
            cmd_rx,
            end.clone(),
        ));
        if !started {
            end.set(End::Failed(FailKind::Runtime));
        }
        Ok(Self {
            reader: Mutex::new(Some(data_rx)),
            cmd: cmd_tx,
            end,
            connection,
        })
    }

    /// O que o servidor mandou. Bloqueia como o PTY bloqueia, e dá fim de
    /// arquivo quando o canal termina. Só a primeira chamada recebe os dados;
    /// as seguintes já nascem no fim.
    pub fn reader(&self) -> ChannelReader {
        let rx = self
            .reader
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        ChannelReader {
            rx,
            pending: Vec::new(),
        }
    }

    /// Nunca bloqueia. Escrever depois do fim dá `BrokenPipe`.
    pub fn writer(&self) -> ChannelWriter {
        ChannelWriter(self.cmd.clone())
    }

    /// Avisa o servidor do novo tamanho (`window-change`). Sem efeito antes de
    /// o canal abrir.
    pub fn resize(&self, rows: u16, cols: u16) {
        let _ = self.cmd.send(Cmd::Resize { rows, cols });
    }

    /// `None` enquanto o canal está aberto ou abrindo.
    pub fn try_end(&self) -> Option<End> {
        self.end.lock().clone()
    }

    /// Cópia de consulta de "ainda aberto", para quem não tem o canal.
    pub fn probe(&self) -> EndProbe {
        EndProbe(self.end.clone())
    }

    /// A conexão deste canal (ADR-0072 §5).
    pub fn connection(&self) -> Option<ConnectionId> {
        Some(self.connection)
    }

    /// Pede o encerramento, sem esperar. Equivale a dropar o canal.
    pub fn close(self) {
        drop(self);
    }
}

impl Drop for SshChannel {
    fn drop(&mut self) {
        // A tarefa guarda uma cópia do `Sender`, então só o fechamento
        // explícito a acorda.
        let _ = self.cmd.send(Cmd::Close);
    }
}

/// `Read` sobre o que a tarefa empurra.
pub struct ChannelReader {
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    pending: Vec<u8>,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            let Some(rx) = &self.rx else {
                return Ok(0);
            };
            match rx.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = self.pending.len().min(buf.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

/// `Write` que entrega os bytes à tarefa.
pub struct ChannelWriter(UnboundedSender<Cmd>);

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .send(Cmd::Data(buf.to_vec()))
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
