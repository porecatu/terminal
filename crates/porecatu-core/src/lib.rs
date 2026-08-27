// SPDX-License-Identifier: GPL-3.0-or-later

//! Modelo de domínio puro (docs/arquitetura.md seção 1, ADR-0006). Sem
//! dependência de nada do projeto -- nem GUI, nem PTY, nem config. É por
//! isso que `porecatu-session` (F5) pode ser um crate trivial: ele
//! serializa `Workspace` e mais nada.
//!
//! `Workspace -> Vec<Group> -> Vec<TabId>`, com o grupo implícito sempre
//! presente e as operações puras do ADR-0006 que a F2 exercita. `serde` é
//! derivado desde já para que o round-trip `Workspace -> JSON -> Workspace`
//! que o ADR lista como invariante seja testável nesta fase, mesmo com
//! `porecatu-session` ainda vazio.

mod group;
mod id;
mod tab;
mod workspace;

pub use group::Group;
pub use id::{GroupId, TabId};
pub use tab::{Tab, TabState};
pub use workspace::Workspace;
