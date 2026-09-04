// SPDX-License-Identifier: GPL-3.0-or-later

//! Dispensa definitiva do convite de integração de shell (ADR-0039 §4). A
//! dispensa é digitada pelo usuário no próprio terminal -- não há widget
//! nem gancho de `Handler` para isso, então este módulo observa os bytes
//! crus do PTY (que já incluem o eco local do que foi digitado, antes do
//! shell terminar de processar a linha) por um marcador literal, do mesmo
//! jeito que `crate::osc7` observa OSC 7 sem depender do motor. Um shell
//! que não reconheça o marcador como comando normalmente responde "comando
//! não encontrado" -- efeito colateral aceito (ADR-0039 §4, "Negativas").

/// Texto que o usuário digita para dispensar o convite em definitivo. A
/// nota (`porecatu-ui`) instrui a digitá-lo; este módulo só precisa
/// reconhecer a substring, então o prefixo que o torna um no-op em cada
/// shell (`:`, `#`, `rem`) não importa aqui.
pub const DISMISS_MARKER: &str = "dispensar-convite-porecatu";

/// Observa bytes crus do PTY e devolve `true` na primeira vez que o
/// marcador aparece. Guarda os últimos `DISMISS_MARKER.len() - 1` bytes
/// entre chamadas -- o marcador pode ficar dividido entre dois lotes de
/// leitura (`Terminal::read_loop` lê em blocos de 4096 bytes).
#[derive(Default)]
pub struct DismissWatcher {
    carry: Vec<u8>,
}

impl DismissWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn advance(&mut self, bytes: &[u8]) -> bool {
        let marker = DISMISS_MARKER.as_bytes();
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(bytes);
        let found = buf.windows(marker.len()).any(|window| window == marker);
        let keep = (marker.len() - 1).min(buf.len());
        self.carry = buf[buf.len() - keep..].to_vec();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_marker_in_a_single_chunk() {
        let mut w = DismissWatcher::new();
        assert!(w.advance(b": dispensar-convite-porecatu\r\n"));
    }

    #[test]
    fn ignores_unrelated_output() {
        let mut w = DismissWatcher::new();
        assert!(!w.advance(b"ls -la\r\ntotal 0\r\n"));
    }

    #[test]
    fn recognizes_the_marker_split_across_two_chunks() {
        let mut w = DismissWatcher::new();
        assert!(!w.advance(b": dispensar-conv"));
        assert!(w.advance(b"ite-porecatu\r\n"));
    }

    #[test]
    fn does_not_refire_on_unrelated_bytes_after_the_carry() {
        let mut w = DismissWatcher::new();
        assert!(!w.advance(b"echo dispensar-conv"));
        assert!(!w.advance(b"ersa qualquer\r\n"));
    }

    #[test]
    fn a_near_miss_does_not_match() {
        let mut w = DismissWatcher::new();
        assert!(!w.advance(b"dispensar-convite-poreca\r\n"));
    }
}
