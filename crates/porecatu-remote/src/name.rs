// SPDX-License-Identifier: GPL-3.0-or-later

//! Name of this machine, as the phone shows it (RF-25.5).

/// Name of the machine from the OS host name, trimmed; `"porecatu"` when the
/// OS gives none or an empty one. Never empty.
pub fn machine_name() -> String {
    let raw = gethostname::gethostname();
    let name = raw.to_string_lossy();
    let name = name.trim();
    if name.is_empty() {
        "porecatu".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_name_is_never_empty() {
        let name = machine_name();
        assert!(!name.is_empty());
        assert_eq!(name, name.trim());
    }
}
