// SPDX-License-Identifier: GPL-3.0-or-later

//! Embute `assets/icon/porecatu.ico` como recurso PE do `.exe` no Windows
//! -- é o que o Explorer, o alt-tab e a barra de tarefas mostram *antes*
//! da janela abrir (o `Icon` do `winit`, aplicado em runtime, só existe
//! depois disso). Sem efeito nas outras duas plataformas da matriz de CI:
//! `winres` só entra como build-dependency sob `cfg(windows)`.

fn main() {
    #[cfg(target_os = "windows")]
    {
        winres::WindowsResource::new()
            .set_icon("assets/icon/porecatu.ico")
            .compile()
            .expect("falha ao embutir assets/icon/porecatu.ico no .exe");
    }
}
