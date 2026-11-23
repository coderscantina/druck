//! The address of a new GitHub issue with the report filled in. The author's browser opens it, and nothing is sent until
//! they press the button there.

const NEW_ISSUE: &str = "https://github.com/coderscantina/druck/issues/new";
/// Browsers and GitHub take addresses of several thousand characters. This stays under the usual 8,000.
const MAX_URL: usize = 7000;

/// What the report says about the machine.
pub struct Facts<'a> {
    pub version: &'a str,
    pub system: &'a str,
}

/// The address, with the last lines of `log` cut to fit. The cut keeps the end of the log, where the problem is.
pub fn url(facts: &Facts, title: &str, log: &str) -> String {
    let head = format!("{NEW_ISSUE}?title={}&body=", encode(title));
    let intro = format!(
        "What happened:\n\n\n\nDruck {}, {}\n\nThe end of the log. The full file is in the Finder window that opened, drag it in here.\n\n```\n",
        facts.version, facts.system
    );
    let outro = "\n```\n";
    let room = MAX_URL.saturating_sub(head.len() + encode(&intro).len() + encode(outro).len());
    let mut kept = log.trim_end();
    // Drop the oldest line until the encoded rest fits. A log line is short, so this takes a few rounds.
    while encode(kept).len() > room {
        kept = kept.split_once('\n').map_or("", |(_, rest)| rest);
    }
    format!("{head}{}", encode(&format!("{intro}{kept}{outro}")))
}

/// Percent-encodes everything but the unreserved characters.
fn encode(text: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(char::from(byte)),
            _ => write!(out, "%{byte:02X}").unwrap_or_default(),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FACTS: Facts = Facts {
        version: "26.11.23",
        system: "macOS 15.1",
    };

    #[test]
    fn text_is_encoded_byte_by_byte() {
        assert_eq!(encode("a b\nä/~"), "a%20b%0A%C3%A4%2F~");
    }

    #[test]
    fn the_address_carries_the_versions_and_the_log() {
        let address = url(&FACTS, "Problem", "line one\nline two\n");
        assert!(address.starts_with("https://github.com/coderscantina/druck/issues/new?title=Problem&body="));
        assert!(address.contains("Druck%2026.11.23%2C%20macOS%2015.1"));
        assert!(address.contains("line%20one%0Aline%20two"));
    }

    #[test]
    fn a_long_log_is_cut_from_its_start_to_fit() {
        let log: String = (0..2000).map(|n| format!("2026-11-23 error number {n}\n")).collect();
        let address = url(&FACTS, "Problem", &log);
        assert!(address.len() <= MAX_URL, "{}", address.len());
        assert!(address.contains("number%201999"));
        assert!(!address.contains("number%200%0A"));
        assert!(address.ends_with("%0A%60%60%60%0A"));
    }
}
