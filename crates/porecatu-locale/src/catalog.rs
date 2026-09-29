// SPDX-License-Identifier: GPL-3.0-or-later

//! Catálogo mesclado (ADR-0056 §6): camadas em ordem, a da direita vence
//! **por chave**. Frase ausente é `None`; quem chama mostra o identificador
//! (RF-15.11).

use std::collections::HashMap;

use crate::layer::Messages;
use crate::message::Message;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    messages: HashMap<String, Message>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mescla `layers` na ordem dada: `[app/en_US, user/en_US, app/L, user/L]`
    /// deixa `user/L` valendo sobre todas.
    pub fn from_layers(layers: impl IntoIterator<Item = Messages>) -> Self {
        let mut catalog = Self::new();
        for layer in layers {
            catalog.merge(layer);
        }
        catalog
    }

    /// Põe `layer` por cima do que já há. A frase inteira é trocada, não
    /// forma a forma: um plural do usuário não herda o `one` do instalado.
    pub fn merge(&mut self, layer: Messages) {
        self.messages.extend(layer);
    }

    pub fn get(&self, id: &str) -> Option<&Message> {
        self.messages.get(id)
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(pairs: &[(&str, &str)]) -> Messages {
        pairs
            .iter()
            .map(|(id, text)| ((*id).to_owned(), Message::Simple((*text).to_owned())))
            .collect()
    }

    fn text(catalog: &Catalog, id: &str) -> Option<String> {
        catalog.get(id).map(|m| m.select(1).to_owned())
    }

    #[test]
    fn right_layer_wins_by_key() {
        let catalog = Catalog::from_layers([
            layer(&[("a", "installed a"), ("b", "installed b")]),
            layer(&[("a", "user a")]),
        ]);
        assert_eq!(text(&catalog, "a").as_deref(), Some("user a"));
        assert_eq!(text(&catalog, "b").as_deref(), Some("installed b"));
    }

    #[test]
    fn chosen_language_wins_over_fallback_and_fills_from_it() {
        let catalog = Catalog::from_layers([
            layer(&[("a", "en a"), ("b", "en b")]),
            layer(&[("a", "pt a")]),
        ]);
        assert_eq!(text(&catalog, "a").as_deref(), Some("pt a"));
        assert_eq!(text(&catalog, "b").as_deref(), Some("en b"));
    }

    #[test]
    fn missing_everywhere_is_none() {
        let catalog = Catalog::from_layers([layer(&[("a", "x")])]);
        assert_eq!(catalog.get("nope"), None);
        assert_eq!(Catalog::new().get("a"), None);
    }

    #[test]
    fn plural_is_replaced_whole() {
        let installed = Message::Plural {
            one: Some("one".to_owned()),
            other: "many".to_owned(),
        };
        let user = Message::Plural {
            one: None,
            other: "user".to_owned(),
        };
        let catalog = Catalog::from_layers([
            Messages::from([("p".to_owned(), installed)]),
            Messages::from([("p".to_owned(), user.clone())]),
        ]);
        assert_eq!(catalog.get("p"), Some(&user));
        assert_eq!(catalog.get("p").unwrap().select(1), "user");
    }
}
