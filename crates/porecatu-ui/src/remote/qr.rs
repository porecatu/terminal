// SPDX-License-Identifier: GPL-3.0-or-later

//! Pairing QR matrix drawn as quads.
//!
//! The matrix comes from the `qrcode` crate at error correction level M; the
//! layout turns it into rectangles whose edges are whole physical pixels, so
//! there is no seam between quads (ADR-0106 §2, ADR-0108 §2): one rectangle for
//! the light background (quiet zone included) and one per horizontal run of
//! dark modules. Colors are the caller's (`qr_dark` and `qr_light`).

use porecatu_render::Rect;
use qrcode::{Color, EcLevel, QrCode};

/// Modules of quiet zone on each side, inside the light background.
const QUIET_ZONE_MODULES: usize = 4;

/// The modules of a QR code, without the quiet zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QrMatrix {
    /// Modules per side, without the quiet zone.
    pub size: usize,
    /// `size * size` modules, row-major; `true` is a dark one.
    dark: Vec<bool>,
}

impl QrMatrix {
    /// Whether the module at `row`, `col` is dark.
    pub(crate) fn is_dark(&self, row: usize, col: usize) -> bool {
        row < self.size && col < self.size && self.dark[row * self.size + col]
    }
}

/// The matrix of `uri`, at error correction level M; `None` when the text does
/// not fit in a QR code.
pub(crate) fn qr_matrix(uri: &str) -> Option<QrMatrix> {
    let code = QrCode::with_error_correction_level(uri.as_bytes(), EcLevel::M).ok()?;
    let size = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|color| color == Color::Dark)
        .collect();
    Some(QrMatrix { size, dark })
}

/// The rectangles of a QR code, in logical pixels with edges on whole physical
/// pixels.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QrLayout {
    /// The light square, quiet zone included.
    pub background: Rect,
    /// One rectangle per horizontal run of dark modules.
    pub dark: Vec<Rect>,
    /// The side of a module, a whole number of physical pixels, in logical
    /// pixels.
    pub module_logical: f32,
}

/// Lays the matrix out inside the square of side `target_side` whose top left
/// corner is `origin` (both logical). The module is the largest whole number of
/// physical pixels that fits (at least 1); the QR is centered in the square and
/// its origin rounded to the physical pixel.
pub(crate) fn qr_layout(
    matrix: &QrMatrix,
    origin: (f32, f32),
    target_side: f32,
    scale_factor: f64,
) -> QrLayout {
    let total_modules = matrix.size + 2 * QUIET_ZONE_MODULES;
    let target_physical = f64::from(target_side) * scale_factor;
    // The epsilon keeps an exact fit (a product that lands a hair below the
    // integer) from losing a whole pixel per module.
    let module = ((target_physical / total_modules as f64 + 1e-6).floor() as i64).max(1);
    let side = module * total_modules as i64;

    let centered = ((target_physical - side as f64) / 2.0).round() as i64;
    let origin_x = (f64::from(origin.0) * scale_factor).round() as i64 + centered;
    let origin_y = (f64::from(origin.1) * scale_factor).round() as i64 + centered;

    let rect = |x: i64, y: i64, width: i64, height: i64| Rect {
        x: (x as f64 / scale_factor) as f32,
        y: (y as f64 / scale_factor) as f32,
        width: (width as f64 / scale_factor) as f32,
        height: (height as f64 / scale_factor) as f32,
    };

    let mut dark = Vec::new();
    for row in 0..matrix.size {
        let mut col = 0;
        while col < matrix.size {
            if !matrix.is_dark(row, col) {
                col += 1;
                continue;
            }
            let start = col;
            while col < matrix.size && matrix.is_dark(row, col) {
                col += 1;
            }
            dark.push(rect(
                origin_x + (QUIET_ZONE_MODULES + start) as i64 * module,
                origin_y + (QUIET_ZONE_MODULES + row) as i64 * module,
                (col - start) as i64 * module,
                module,
            ));
        }
    }

    QrLayout {
        background: rect(origin_x, origin_y, side, side),
        dark,
        module_logical: (module as f64 / scale_factor) as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "porecatu://pair?v=0&relay=wss%3A%2F%2Frelay.example.com&host=abcdefghijklmnopqrstuvwxyz234567&key=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&token=AAAAAAAAAAAAAAAAAAAAAA&name=Desk";

    fn physical(value: f32, scale: f64) -> f64 {
        f64::from(value) * scale
    }

    fn assert_whole(value: f32, scale: f64) {
        let px = physical(value, scale);
        assert!(
            (px - px.round()).abs() < 1e-3,
            "{value} * {scale} = {px} is not a whole pixel"
        );
    }

    fn assert_rect_whole(rect: &Rect, scale: f64) {
        for value in [rect.x, rect.y, rect.width, rect.height] {
            assert_whole(value, scale);
        }
    }

    #[test]
    fn pairing_uri_makes_a_matrix() {
        let matrix = qr_matrix(URI).expect("fits");
        assert!(matrix.size >= 21);
        assert_eq!((matrix.size - 21) % 4, 0, "size is 17 + 4 * version");
        assert_eq!(matrix.dark.len(), matrix.size * matrix.size);
        assert!(matrix.dark.iter().any(|dark| *dark));
        assert!(matrix.dark.iter().any(|dark| !*dark));
        // The top left finder pattern: corner dark, ring inside is light.
        assert!(matrix.is_dark(0, 0));
        assert!(matrix.is_dark(0, 6));
        assert!(!matrix.is_dark(1, 1));
        assert!(matrix.is_dark(2, 2));
    }

    #[test]
    fn text_too_long_gives_none() {
        assert!(qr_matrix(&"x".repeat(5000)).is_none());
    }

    #[test]
    fn level_m_is_used() {
        // Level M holds fewer bytes than L at the same version: a payload that
        // fits at version 1 with L (17 bytes) needs a larger one at M.
        let matrix = qr_matrix("abcdefghijklmnopq").expect("fits");
        assert!(matrix.size > 21);
    }

    fn check_layout(scale: f64) {
        let matrix = qr_matrix(URI).expect("fits");
        let target = 208.0_f32;
        let origin = (13.3_f32, 7.7_f32);
        let layout = qr_layout(&matrix, origin, target, scale);

        // Whole physical pixels everywhere.
        assert_rect_whole(&layout.background, scale);
        assert_whole(layout.module_logical, scale);
        for rect in &layout.dark {
            assert_rect_whole(rect, scale);
        }

        // The side never goes past the target.
        assert!(layout.background.width <= target + 1e-3);
        assert_eq!(layout.background.width, layout.background.height);

        // Module is a whole number of physical pixels, the largest that fits.
        let module = physical(layout.module_logical, scale).round();
        let total = (matrix.size + 8) as f64;
        assert!(module >= 1.0);
        assert!(module * total <= (f64::from(target) * scale) + 1e-3);
        assert!((module + 1.0) * total > f64::from(target) * scale);

        // Inside the target square.
        assert!(layout.background.x >= origin.0 - 1.0);
        assert!(layout.background.y >= origin.1 - 1.0);
        assert!(layout.background.x + layout.background.width <= origin.0 + target + 1.0);
        assert!(layout.background.y + layout.background.height <= origin.1 + target + 1.0);

        // Every dark rectangle is inside the matrix area, inside the quiet zone.
        let quiet = 4.0 * physical(layout.module_logical, scale);
        for rect in &layout.dark {
            assert!(physical(rect.x, scale) >= physical(layout.background.x, scale) + quiet - 1e-3);
            assert!(
                physical(rect.x + rect.width, scale)
                    <= physical(layout.background.x + layout.background.width, scale) - quiet
                        + 1e-3
            );
        }

        // Rectangles are in physical pixels from here on.
        let phys: Vec<(i64, i64, i64, i64)> = layout
            .dark
            .iter()
            .map(|r| {
                (
                    physical(r.x, scale).round() as i64,
                    physical(r.y, scale).round() as i64,
                    physical(r.width, scale).round() as i64,
                    physical(r.height, scale).round() as i64,
                )
            })
            .collect();

        // No two rectangles overlap.
        for (i, a) in phys.iter().enumerate() {
            for b in &phys[i + 1..] {
                let overlap_x = a.0 < b.0 + b.2 && b.0 < a.0 + a.2;
                let overlap_y = a.1 < b.1 + b.3 && b.1 < a.1 + a.3;
                assert!(!(overlap_x && overlap_y), "{a:?} overlaps {b:?}");
            }
        }

        // The modules covered are exactly the dark ones of the matrix.
        let covered: i64 = phys
            .iter()
            .map(|r| (r.2 / module as i64) * (r.3 / module as i64))
            .sum();
        let dark_modules = matrix.dark.iter().filter(|dark| **dark).count() as i64;
        assert_eq!(covered, dark_modules);

        // And each rectangle is one maximal horizontal run.
        let bg_x = physical(layout.background.x, scale).round() as i64;
        let bg_y = physical(layout.background.y, scale).round() as i64;
        let m = module as i64;
        let mut seen = vec![false; matrix.size * matrix.size];
        for (x, y, w, _) in &phys {
            let col = ((x - bg_x) / m - 4) as usize;
            let row = ((y - bg_y) / m - 4) as usize;
            let run = (w / m) as usize;
            for c in col..col + run {
                assert!(matrix.is_dark(row, c));
                seen[row * matrix.size + c] = true;
            }
            if col > 0 {
                assert!(
                    !matrix.is_dark(row, col - 1),
                    "run does not start at its edge"
                );
            }
            if col + run < matrix.size {
                assert!(
                    !matrix.is_dark(row, col + run),
                    "run does not reach its edge"
                );
            }
        }
        for row in 0..matrix.size {
            for col in 0..matrix.size {
                assert_eq!(seen[row * matrix.size + col], matrix.is_dark(row, col));
            }
        }
    }

    #[test]
    fn layout_has_whole_pixel_edges_at_scale_1() {
        check_layout(1.0);
    }

    #[test]
    fn layout_has_whole_pixel_edges_at_scale_1_25() {
        check_layout(1.25);
    }

    #[test]
    fn layout_has_whole_pixel_edges_at_scale_1_5() {
        check_layout(1.5);
    }

    #[test]
    fn layout_has_whole_pixel_edges_at_scale_2() {
        check_layout(2.0);
    }

    #[test]
    fn runs_are_fewer_than_dark_modules() {
        let matrix = qr_matrix(URI).expect("fits");
        let layout = qr_layout(&matrix, (0.0, 0.0), 208.0, 1.0);
        let dark_modules = matrix.dark.iter().filter(|dark| **dark).count();
        assert!(layout.dark.len() < dark_modules);
    }

    #[test]
    fn tiny_target_still_gets_a_one_pixel_module() {
        let matrix = QrMatrix {
            size: 21,
            dark: vec![true; 21 * 21],
        };
        let layout = qr_layout(&matrix, (0.0, 0.0), 10.0, 1.0);
        assert_eq!(layout.module_logical, 1.0);
        assert_eq!(layout.background.width, 29.0);
        // One run per row, as every module is dark.
        assert_eq!(layout.dark.len(), 21);
    }

    #[test]
    fn exact_fit_keeps_every_pixel() {
        // 29 modules at 4 physical pixels: 116 physical = 92.8 logical at 1.25
        // would be inexact; 116 / 2.0 = 58 is exact.
        let matrix = QrMatrix {
            size: 21,
            dark: vec![false; 21 * 21],
        };
        let layout = qr_layout(&matrix, (0.0, 0.0), 58.0, 2.0);
        assert_eq!(layout.module_logical, 2.0);
        assert_eq!(layout.background.width, 58.0);
        assert_eq!(layout.background.x, 0.0);
    }

    #[test]
    fn qr_is_centered_in_the_target_square() {
        let matrix = QrMatrix {
            size: 21,
            dark: vec![false; 21 * 21],
        };
        // 29 modules; target 100 -> module 3 -> side 87; 13 spare, 6 or 7 to
        // the left (rounded).
        let layout = qr_layout(&matrix, (10.0, 20.0), 100.0, 1.0);
        assert_eq!(layout.background.width, 87.0);
        assert_eq!(layout.background.x, 17.0);
        assert_eq!(layout.background.y, 27.0);
    }
}
