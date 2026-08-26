// SPDX-License-Identifier: GPL-3.0-or-later

//! Clipboard do sistema, encapsulado num único lugar (ADR-0013). Plano B
//! registrado no ADR é `copypasta`, caso o caminho do Wayland precise de
//! um handle do `winit` que o `arboard` não dê conta -- **não verificado
//! nesta etapa**: sem ambiente Linux/Wayland disponível para testar (ver
//! relatório da Etapa 6, "não consegui verificar").
//!
//! `arboard::Clipboard` é recriado a cada chamada em vez de guardado num
//! campo -- mais simples, e evita a dúvida de manter viva uma instância
//! entre frames num crate que ainda não tem onde armazenar isso sem
//! acoplar ao resto do `App`. Custo de abrir/fechar a cada cópia/colagem é
//! desprezível perto da frequência dessas ações.

pub fn copy(text: &str) {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            if let Err(err) = clipboard.set_text(text) {
                eprintln!("porecatu: falha ao copiar para o clipboard: {err}");
            }
        }
        Err(err) => eprintln!("porecatu: clipboard indisponível: {err}"),
    }
}

pub fn paste() -> Option<String> {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => match clipboard.get_text() {
            Ok(text) => Some(text),
            Err(err) => {
                eprintln!("porecatu: falha ao ler o clipboard: {err}");
                None
            }
        },
        Err(err) => {
            eprintln!("porecatu: clipboard indisponível: {err}");
            None
        }
    }
}
