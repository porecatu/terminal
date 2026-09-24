// SPDX-License-Identifier: GPL-3.0-or-later

//! Schema v1 do arquivo de sessão (ADR-0036 §1). `schema_version` nasce em
//! **1**. `#[serde(default)]` no container e nos campos opcionais é o que
//! permite acrescentar campo opcional depois sem subir a versão.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionFileV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub windows: Vec<WindowV1>,
    /// Dispensa definitiva do convite de integração de shell (ADR-0039).
    #[serde(default)]
    pub shell_integration_dismissed: bool,
    /// Nome exibido (ADR-0054 §3) -- só sessões nomeadas; o `session.json`
    /// automático nunca preenche, e por isso omitido da saída quando
    /// ausente (`skip_serializing_if`), para o arquivo automático
    /// continuar byte a byte igual ao de antes deste campo existir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Segundos desde a época Unix, gravado no gesto de salvar (ADR-0054
    /// §4) -- não é o `mtime` do arquivo, que muda por cópia ou
    /// sincronização de pasta. Mesmo tratamento de ausência que `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowV1 {
    pub geometry: GeometryV1,
    pub monitor: Option<MonitorIdV1>,
    pub groups: Vec<GroupV1>,
    pub tabs: Vec<TabV1>,
    pub active_tab: Option<u32>,
    /// Tema de sessão, por janela (ADR-0031, ADR-0036 §3).
    pub theme: Option<String>,
    /// Passos de zoom de sessão, por janela.
    pub zoom_steps: i32,
}

/// Um grupo, na ordem em que aparece na barra. Implícito quando `name` e
/// `color` são `None` -- mesmo discriminante do domínio (`GroupKind`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupV1 {
    pub id: u32,
    pub name: Option<String>,
    pub color: Option<String>,
    pub collapsed: bool,
    pub tabs: Vec<u32>,
}

/// Uma aba. `spawn_program` é o shell/programa que a spawnou -- gravado
/// para diferenciar do shell padrão da config na restauração. `cwd`/
/// `spawn_program` descrevem o painel **focado**, para o arquivo continuar
/// legível por uma versão anterior a esta (ADR-0036 §3 revisto pelo
/// ADR-0053 §11) -- `panes` é a fonte completa, quando presente.
// `Eq` não dá para derivar: `panes` carrega `ratio: f32` (ADR-0053 §11),
// que não é `Eq`. `PartialEq` continua -- é o que os testes de round-trip
// usam.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TabV1 {
    pub id: u32,
    pub custom_title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub spawn_program: Option<String>,
    /// ADR-0053 §11: a árvore de painéis da aba -- estrutura, `ratio` de
    /// cada divisor, `cwd` e programa de cada painel, e qual estava
    /// focado. **Opcional, sem subir `schema_version`**: ausência (arquivo
    /// gravado por uma versão anterior) significa "um painel só", e quem
    /// decide isso é `porecatu_session::convert::workspace_from_window`,
    /// caindo de volta em `cwd`/`spawn_program` acima.
    pub panes: Option<PaneTreeV1>,
}

/// Um painel dentro da árvore (ADR-0053 §11) -- o que era "por aba, `cwd` e
/// programa de spawn" antes dos painéis existirem. `id` é o identificador
/// que o arquivo usa para marcar qual folha é a focada
/// (`PaneTreeV1::focused`); não sobrevive à leitura, como `TabV1::id`
/// também não (`porecatu_session::convert`) -- a restauração gera
/// `PaneId` novo, na ordem de travessia da árvore.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneV1 {
    pub id: u32,
    pub cwd: Option<PathBuf>,
    pub spawn_program: Option<String>,
}

/// Eixo do divisor, espelhando `porecatu_core::SplitAxis` -- tipo próprio
/// em vez de reexportar o do domínio, pela mesma razão do resto do schema
/// (ADR-0036 §2): o formato de disco não acopla à forma do domínio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxisV1 {
    Horizontal,
    Vertical,
}

/// Nó da árvore de painéis gravada -- folha com o painel, ou divisor com
/// eixo, proporção e os dois filhos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaneNodeV1 {
    Leaf(PaneV1),
    Split {
        axis: SplitAxisV1,
        ratio: f32,
        first: Box<PaneNodeV1>,
        second: Box<PaneNodeV1>,
    },
}

/// A árvore de painéis de uma aba (ADR-0053 §11), com o `id` (de
/// `PaneV1::id`) do painel que estava focado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneTreeV1 {
    pub root: PaneNodeV1,
    pub focused: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GeometryV1 {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorIdV1 {
    #[serde(default)]
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
}
