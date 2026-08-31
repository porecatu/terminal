// SPDX-License-Identifier: GPL-3.0-or-later

//! Ícone da janela -- taskbar/titlebar no Windows e X11 (`winit` não
//! aplica em Wayland nem macOS, que tomam o ícone do bundle do app, ainda
//! não empacotado nesta fase). Decodificado uma vez do PNG embutido.
//!
//! Não é o ícone do `.exe` visto no Explorer/alt-tab antes da janela
//! abrir -- esse é um recurso PE separado, embutido pelo `build.rs` do
//! bin `porecatu` a partir do mesmo desenho (`assets/icon/porecatu.ico`).

use winit::window::Icon;

const PNG_BYTES: &[u8] = include_bytes!("../../../assets/icon/porecatu.png");

/// Decodifica o ícone embutido. Painica em PNG malformado ou fora do
/// RGBA8 esperado -- é asset do binário, não entrada do usuário, e um
/// `assets/icon/porecatu.png` corrompido é bug de build, não algo a
/// degradar silenciosamente.
pub fn load() -> Icon {
    let decoder = png::Decoder::new(PNG_BYTES);
    let mut reader = decoder
        .read_info()
        .expect("assets/icon/porecatu.png deveria decodificar");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .expect("assets/icon/porecatu.png deveria decodificar");
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "assets/icon/porecatu.png deveria ser RGBA8"
    );
    assert_eq!(
        info.bit_depth,
        png::BitDepth::Eight,
        "assets/icon/porecatu.png deveria ser RGBA8"
    );
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).expect("ícone embutido deveria ser válido")
}
