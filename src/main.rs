// SPDX-License-Identifier: GPL-3.0-or-later

// Sem console junto da janela no Windows -- sem efeito nos outros alvos
// (a diretiva é ignorada fora de `windows`), então não precisa de `cfg`.
#![windows_subsystem = "windows"]

fn main() {
    porecatu_ui::run();
}
