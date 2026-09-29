// SPDX-License-Identifier: GPL-3.0-or-later

//! Catálogo de textos da interface (ADR-0056, PRD-015).
//!
//! Crate folha: lê, valida e mescla arquivos TOML de idioma achados em disco.
//! Não conhece quais frases o app tem (o esquema chega de fora), não lê
//! `std::env` nem `current_exe` (os diretórios chegam por argumento) e não
//! produz prosa de interface -- a única frase é [`NO_CATALOG_MESSAGE`].

mod catalog;
mod layer;
mod message;
mod name;
mod resolve;

pub use catalog::Catalog;
pub use layer::{
    InvalidKey, InvalidReason, LayerOutcome, MessageSpec, Messages, Schema, SyntaxError,
    parse_layer,
};
pub use message::{Message, PluralForm, format, plural_form};
pub use name::{FALLBACK_LOCALE, InvalidLocaleName, LocaleName};
pub use resolve::{CatalogOutcome, Diagnostic, NO_CATALOG_MESSAGE, build_catalog};
