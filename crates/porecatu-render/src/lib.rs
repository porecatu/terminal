// SPDX-License-Identifier: GPL-3.0-or-later

//! Renderer `wgpu`/`glyphon` (docs/arquitetura.md seção 5, ADR-0018). Não
//! conhece domínio -- recebe [`Frame`]s com primitivas prontas (quad,
//! retângulo arredondado, run de texto, clip), nada sobre aba ou grupo;
//! quem traduz é `porecatu-ui`.
//!
//! Dividido em [`GpuContext`] (`Instance`/`Device`/`Queue`/atlas/pipeline,
//! um por processo) e [`WindowSurface`] (surface/`Viewport`/escala, um por
//! janela) -- é o que permite duas janelas compartilharem GPU e atlas de
//! glyphs (ADR-0015). [`TextMeasurer`] mede texto sem `Device` nem
//! `Queue`, o que torna o layout de chrome uma função pura testável sem
//! GPU (seção 7 da arquitetura).

mod frame;
mod gpu;
pub mod icon;
mod primitives;
mod quad;
mod text;
mod text_measurer;
mod window_surface;

pub use frame::{Frame, Layer};
pub use gpu::{GpuContext, SurfaceError};
pub use primitives::{Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextRun};
pub use text_measurer::TextMeasurer;
pub use window_surface::WindowSurface;
