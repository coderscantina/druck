//! Escaping text for XML documents. Characters XML 1.0 forbids, such as most control characters, are dropped.

/// `text` escaped for element content.
pub fn text(text: &str) -> String {
    escape(text, false)
}

/// `value` escaped for a double-quoted attribute value.
pub fn attribute(value: &str) -> String {
    escape(value, true)
}

fn escape(input: &str, attribute: bool) -> String {
    let mut escaped = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' if attribute => escaped.push_str("&quot;"),
            // Attribute values normalize line breaks and tabs to spaces unless written as references.
            '\n' | '\t' | '\r' if attribute => escaped.push_str(&format!("&#{};", u32::from(character))),
            character if allowed(character) => escaped.push(character),
            _ => {}
        }
    }
    escaped
}

/// Whether XML 1.0 allows `character` in a document.
fn allowed(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_and_drops_characters_xml_forbids() {
        assert_eq!(text("a < b & c > \"d\"\u{0}\u{1b}"), "a &lt; b &amp; c &gt; \"d\"");
        assert_eq!(
            attribute("say \"hi\" & <go>\nnow"),
            "say &quot;hi&quot; &amp; &lt;go&gt;&#10;now"
        );
        assert_eq!(text("Ünïcödé 🙂"), "Ünïcödé 🙂");
    }
}
