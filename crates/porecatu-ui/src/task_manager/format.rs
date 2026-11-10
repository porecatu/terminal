// SPDX-License-Identifier: GPL-3.0-or-later

//! Localized number formatting of the task manager (ADR-0095 §6, ADR-0096
//! §2 and §4). Units are base 1024 (B, KB, MB, GB), one decimal from KB on,
//! integer percentages, disk with a per-second suffix. The unit symbols, the
//! decimal separator, the percent and per-second shapes and the "no value"
//! dash all come from the catalog: `1,4 GB` in pt_BR is `1.4 GB` in en_US.

use porecatu_locale::Catalog;

use crate::messages::msg;

use super::chart::Metric;
use super::rows::NoteKind;

/// Bytes per display unit: B, KB, MB, GB.
const UNITS: [f64; 4] = [1.0, 1024.0, 1_048_576.0, 1_073_741_824.0];

/// The unit symbol at `index` of [`UNITS`].
fn unit_symbol(catalog: &Catalog, index: usize) -> String {
    match index {
        0 => msg::task_manager::unit_b(catalog),
        1 => msg::task_manager::unit_kb(catalog),
        2 => msg::task_manager::unit_mb(catalog),
        _ => msg::task_manager::unit_gb(catalog),
    }
}

/// `value` with `places` decimals and the catalog's decimal separator.
fn decimal(catalog: &Catalog, value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if places == 0 {
        return text;
    }
    text.replace('.', &msg::task_manager::decimal_separator(catalog))
}

/// The unit and the scaled value for `bytes`: the largest unit the value
/// reaches, promoted once more when the rounded number would read 1024.
/// `places` is the decimals the number is shown with.
fn scale(bytes: f64, places_from_kb: usize) -> (usize, f64, usize) {
    let bytes = if bytes.is_finite() && bytes > 0.0 {
        bytes
    } else {
        0.0
    };
    let mut unit = UNITS.iter().rposition(|size| bytes >= *size).unwrap_or(0);
    loop {
        let places = if unit == 0 { 0 } else { places_from_kb };
        let value = bytes / UNITS[unit];
        let factor = 10f64.powi(places as i32);
        let rounded = (value * factor).round() / factor;
        if rounded >= 1024.0 && unit + 1 < UNITS.len() {
            unit += 1;
            continue;
        }
        return (unit, value, places);
    }
}

/// A byte count: `612 B`, `74,0 KB`, `1,4 GB`.
pub(crate) fn bytes(catalog: &Catalog, value: u64) -> String {
    bytes_f64(catalog, value as f64)
}

fn bytes_f64(catalog: &Catalog, value: f64) -> String {
    let (unit, scaled, places) = scale(value, 1);
    format!(
        "{} {}",
        decimal(catalog, scaled, places),
        unit_symbol(catalog, unit)
    )
}

/// A rate in bytes per second: `0 B/s`, `1,2 MB/s`.
pub(crate) fn rate(catalog: &Catalog, value: f64) -> String {
    msg::task_manager::per_second(catalog, bytes_f64(catalog, value))
}

/// CPU as an integer percentage of the machine (`12%`); `None` is the "no
/// value" dash of the catalog (the sampler's first reading, RF-24.9).
pub(crate) fn percent(catalog: &Catalog, value: Option<f32>) -> String {
    match value {
        Some(fraction) => {
            let whole = if fraction.is_finite() {
                (fraction * 100.0).round().max(0.0) as u32
            } else {
                0
            };
            msg::task_manager::percent(catalog, whole)
        }
        None => msg::task_manager::no_value(catalog),
    }
}

/// The label of the top of a chart scale (ADR-0096 §2): `100%`, `2 GB`,
/// `5 MB/s`. Without a decimal when the number is whole, which the steps of
/// `chart::nice_top` always are.
pub(crate) fn top_label(catalog: &Catalog, metric: Metric, top: f64) -> String {
    match metric {
        Metric::Cpu => percent(catalog, Some(top as f32)),
        Metric::Memory => whole_bytes(catalog, top),
        Metric::Disk => msg::task_manager::per_second(catalog, whole_bytes(catalog, top)),
    }
}

/// Like [`bytes_f64`] but with no decimal when the scaled number is whole.
fn whole_bytes(catalog: &Catalog, value: f64) -> String {
    let (unit, scaled, places) = scale(value, 1);
    let is_whole = (scaled - scaled.round()).abs() < 1e-9;
    let places = if is_whole { 0 } else { places };
    format!(
        "{} {}",
        decimal(catalog, scaled, places),
        unit_symbol(catalog, unit)
    )
}

/// The widest text each numeric column can show (ADR-0096 §4), with the
/// language's separator. The painting measures them **once** per font size
/// and language to size the columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnModels {
    pub pid: String,
    pub cpu: String,
    pub memory: String,
    pub disk: String,
}

pub(crate) fn column_models(catalog: &Catalog) -> ColumnModels {
    let memory = format!(
        "8888{}8 {}",
        msg::task_manager::decimal_separator(catalog),
        msg::task_manager::unit_mb(catalog)
    );
    ColumnModels {
        pid: "8888888".to_owned(),
        cpu: msg::task_manager::percent(catalog, 100),
        disk: msg::task_manager::per_second(catalog, &memory),
        memory,
    }
}

/// The title of a chart (ADR-0096 §2).
pub(crate) fn chart_title(catalog: &Catalog, metric: Metric) -> String {
    match metric {
        Metric::Cpu => msg::task_manager::chart_cpu(catalog),
        Metric::Memory => msg::task_manager::chart_memory(catalog),
        Metric::Disk => msg::task_manager::chart_disk(catalog),
    }
}

/// The column headers of the list (ADR-0096 §4), as the catalog spells them;
/// the painting upper-cases them for the section-label style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnHeaders {
    pub name: String,
    pub pid: String,
    pub cpu: String,
    pub memory: String,
    pub disk: String,
}

pub(crate) fn column_headers(catalog: &Catalog) -> ColumnHeaders {
    ColumnHeaders {
        name: msg::task_manager::column_name(catalog),
        pid: msg::task_manager::column_pid(catalog),
        cpu: msg::task_manager::column_cpu(catalog),
        memory: msg::task_manager::column_memory(catalog),
        disk: msg::task_manager::column_disk(catalog),
    }
}

/// The text of a note row (RF-24.17).
pub(crate) fn note_text(catalog: &Catalog, kind: NoteKind) -> String {
    match kind {
        NoteKind::NotStarted => msg::task_manager::note_not_started(catalog),
        NoteKind::Ssh => msg::task_manager::note_ssh(catalog),
        NoteKind::Exited => msg::task_manager::note_exited(catalog),
        NoteKind::Waiting => msg::task_manager::note_waiting(catalog),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support::{all_catalogs, en_us, pt_br};

    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn bytes_in_pt_br_use_a_comma_and_one_decimal_from_kb() {
        let c = pt_br();
        assert_eq!(bytes(&c, 0), "0 B");
        assert_eq!(bytes(&c, 612), "612 B");
        assert_eq!(bytes(&c, KB), "1,0 KB");
        assert_eq!(bytes(&c, 74 * MB), "74,0 MB");
        assert_eq!(bytes(&c, GB + 400 * MB), "1,4 GB");
        assert_eq!(bytes(&c, 8888 * MB + 800 * KB), "8,7 GB");
    }

    #[test]
    fn bytes_in_en_us_use_a_dot() {
        let c = en_us();
        assert_eq!(bytes(&c, 612), "612 B");
        assert_eq!(bytes(&c, GB + 400 * MB), "1.4 GB");
        assert_eq!(bytes(&c, 3 * KB / 2), "1.5 KB");
    }

    #[test]
    fn a_number_that_rounds_to_1024_moves_to_the_next_unit() {
        let c = en_us();
        // 1023.96 KB reads 1024.0 KB, so it is 1.0 MB.
        assert_eq!(bytes(&c, MB - 40), "1.0 MB");
        // 1023.6 B reads 1024 B, so it is 1.0 KB.
        assert_eq!(rate(&c, 1023.6), "1.0 KB/s");
        // GB is the last unit.
        assert_eq!(bytes(&c, 5000 * GB), "5000.0 GB");
    }

    #[test]
    fn rate_adds_the_per_second_suffix() {
        assert_eq!(rate(&pt_br(), 0.0), "0 B/s");
        assert_eq!(rate(&pt_br(), 1.5 * MB as f64), "1,5 MB/s");
        assert_eq!(rate(&en_us(), 1.5 * MB as f64), "1.5 MB/s");
        // Junk never panics or prints a sign.
        assert_eq!(rate(&en_us(), -3.0), "0 B/s");
        assert_eq!(rate(&en_us(), f64::NAN), "0 B/s");
    }

    #[test]
    fn percent_is_an_integer_and_none_is_the_dash() {
        for c in [pt_br(), en_us()] {
            assert_eq!(percent(&c, Some(0.0)), "0%");
            assert_eq!(percent(&c, Some(0.124)), "12%");
            assert_eq!(percent(&c, Some(0.125)), "13%");
            assert_eq!(percent(&c, Some(1.0)), "100%");
            assert_eq!(percent(&c, None), "\u{2014}");
        }
        assert_eq!(percent(&pt_br(), Some(f32::NAN)), "0%");
    }

    #[test]
    fn top_labels_are_whole_numbers_when_they_can_be() {
        let c = pt_br();
        assert_eq!(top_label(&c, Metric::Cpu, 1.0), "100%");
        assert_eq!(top_label(&c, Metric::Memory, 2.0 * GB as f64), "2 GB");
        assert_eq!(top_label(&c, Metric::Memory, 1000.0 * MB as f64), "1000 MB");
        assert_eq!(top_label(&c, Metric::Disk, 5.0 * MB as f64), "5 MB/s");
        assert_eq!(top_label(&c, Metric::Disk, KB as f64), "1 KB/s");
        // A top that is not whole keeps the decimal, with the separator.
        assert_eq!(top_label(&c, Metric::Memory, 1.5 * GB as f64), "1,5 GB");
        assert_eq!(
            top_label(&en_us(), Metric::Memory, 1.5 * GB as f64),
            "1.5 GB"
        );
    }

    #[test]
    fn column_models_follow_the_language_separator() {
        let pt = column_models(&pt_br());
        assert_eq!(pt.pid, "8888888");
        assert_eq!(pt.cpu, "100%");
        assert_eq!(pt.memory, "8888,8 MB");
        assert_eq!(pt.disk, "8888,8 MB/s");
        let en = column_models(&en_us());
        assert_eq!(en.memory, "8888.8 MB");
        assert_eq!(en.disk, "8888.8 MB/s");
    }

    #[test]
    fn titles_headers_and_notes_come_from_the_catalog_in_pt_br() {
        let c = pt_br();
        assert_eq!(chart_title(&c, Metric::Cpu), "CPU");
        assert_eq!(chart_title(&c, Metric::Memory), "Memória");
        assert_eq!(chart_title(&c, Metric::Disk), "Disco");
        let headers = column_headers(&c);
        assert_eq!(
            [
                headers.name,
                headers.pid,
                headers.cpu,
                headers.memory,
                headers.disk
            ],
            ["Nome", "PID", "CPU", "Memória", "Disco"]
        );
        assert_eq!(
            note_text(&c, NoteKind::NotStarted),
            "A aba inicia no primeiro foco."
        );
        assert_eq!(
            note_text(&c, NoteKind::Ssh),
            "Os processos remotos não são listados."
        );
        assert_eq!(
            note_text(&c, NoteKind::Exited),
            "O processo do painel terminou."
        );
        assert_eq!(
            note_text(&c, NoteKind::Waiting),
            "Aguardando a primeira medição…"
        );
    }

    #[test]
    fn every_language_has_every_title_header_and_note() {
        for catalog in all_catalogs() {
            let mut texts = vec![
                chart_title(&catalog, Metric::Cpu),
                chart_title(&catalog, Metric::Memory),
                chart_title(&catalog, Metric::Disk),
            ];
            let headers = column_headers(&catalog);
            texts.extend([
                headers.name,
                headers.pid,
                headers.cpu,
                headers.memory,
                headers.disk,
            ]);
            for kind in [
                NoteKind::NotStarted,
                NoteKind::Ssh,
                NoteKind::Exited,
                NoteKind::Waiting,
            ] {
                texts.push(note_text(&catalog, kind));
            }
            for text in texts {
                assert!(!text.is_empty());
                assert!(!text.starts_with("task_manager."), "{text}");
            }
        }
    }

    #[test]
    fn every_shipped_language_formats_without_leaking_an_identifier() {
        for catalog in all_catalogs() {
            let samples = [
                bytes(&catalog, 612),
                bytes(&catalog, GB + 400 * MB),
                rate(&catalog, 1.5 * MB as f64),
                percent(&catalog, Some(0.5)),
                percent(&catalog, None),
                top_label(&catalog, Metric::Memory, 2.0 * GB as f64),
                top_label(&catalog, Metric::Disk, 5.0 * MB as f64),
            ];
            for text in samples {
                assert!(!text.contains("task_manager."), "{text}");
                assert!(!text.contains('{'), "{text}");
            }
        }
    }
}
