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
/// para diferenciar do shell padrão da config na restauração.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TabV1 {
    pub id: u32,
    pub custom_title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub spawn_program: Option<String>,
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
