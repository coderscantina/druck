//! The address of a new GitHub issue with the report filled in. The author's browser opens it, and nothing is sent until
//! they press the button there. The address carries only the versions: a browser sends it as the form loads, and the
//! log can quote the book, so the author reads the log and attaches it themselves.

const NEW_ISSUE: &str = "https://github.com/coderscantina/druck/issues/new";

/// What the report says about the machine.
pub struct Facts<'a> {
    pub version: &'a str,
    pub system: &'a str,
}

/// The address, with the versions and a line asking for the log.
pub fn url(facts: &Facts, title: &str) -> String {
    let body = format!(
        "What happened:\n\n\n\nDruck {}, {}\n\nThe log, druck.log, is in the Finder window that opened. Read it, then drag it in here if it holds nothing you want to keep private.\n",
        facts.version, facts.system
    );
    format!("{NEW_ISSUE}?title={}&body={}", encode(title), encode(&body))
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
    fn the_address_carries_the_versions_and_asks_for_the_log() {
        let address = url(&FACTS, "Problem");
        assert!(address.starts_with("https://github.com/coderscantina/druck/issues/new?title=Problem&body="));
        assert!(address.contains("Druck%2026.11.23%2C%20macOS%2015.1"));
        assert!(address.contains("druck.log"));
        assert!(address.len() < 600, "{}", address.len());
    }
}
