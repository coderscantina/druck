//! Converts the LaTeX escapes common in German and English BibTeX text to Unicode.

use std::iter::Peekable;
use std::str::Chars;

/// Accented letters by accent and base letter, covering German and the usual English loan words.
const ACCENTED: &[(char, &str, &str)] = &[
    ('"', "aouAOUeEiI", "äöüÄÖÜëËïÏ"),
    ('\'', "aeiouAEIOUy", "áéíóúÁÉÍÓÚý"),
    ('`', "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
    ('^', "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
];

/// Control words that stand for one character.
const WORDS: &[(&str, char)] = &[("ss", 'ß')];

/// Converts BibTeX text: escapes become characters, protective braces disappear, `--` and `---` become dashes, `~`
/// becomes a no-break space, and whitespace collapses. Anything else after a backslash is an error message that
/// names the offending escape.
pub fn text(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => escape(&mut chars, &mut out)?,
            '{' | '}' => {}
            '~' => out.push('\u{a0}'),
            '-' => {
                let mut run = 1;
                while chars.next_if_eq(&'-').is_some() {
                    run += 1;
                }
                match run {
                    2 => out.push('–'),
                    3 => out.push('—'),
                    n => out.extend(std::iter::repeat_n('-', n)),
                }
            }
            c if c.is_whitespace() => {
                while chars.next_if(|c| c.is_whitespace()).is_some() {}
                out.push(' ');
            }
            c => out.push(c),
        }
    }
    Ok(out.trim().to_owned())
}

/// Reads a URL or DOI: braces and whitespace disappear and `\_`, `\%`, `\&`, `\#` lose their backslash.
pub fn verbatim(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '{' | '}' => {}
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

fn escape(chars: &mut Peekable<Chars<'_>>, out: &mut String) -> Result<(), String> {
    let Some(next) = chars.next() else {
        return Err("a backslash at the end of the text".to_owned());
    };
    if let Some((_, bases, accented)) = ACCENTED.iter().find(|(accent, ..)| *accent == next) {
        let braced = chars.next_if_eq(&'{').is_some();
        let letter = chars
            .next()
            .ok_or_else(|| format!("the accent `\\{next}` without a letter"))?;
        if braced && chars.next() != Some('}') {
            return Err(format!("the accent `\\{next}` with more than one letter"));
        }
        let converted = bases
            .chars()
            .position(|b| b == letter)
            .and_then(|p| accented.chars().nth(p));
        out.push(converted.ok_or_else(|| format!("the accent `\\{next}{letter}`"))?);
        return Ok(());
    }
    match next {
        '&' | '%' | '$' | '#' | '_' | '{' | '}' | ' ' => out.push(next),
        c if c.is_ascii_alphabetic() => {
            let mut name = c.to_string();
            while let Some(letter) = chars.next_if(char::is_ascii_alphabetic) {
                name.push(letter);
            }
            let (_, converted) = WORDS
                .iter()
                .find(|(word, _)| *word == name)
                .ok_or_else(|| format!("the LaTeX command `\\{name}`"))?;
            out.push(*converted);
            // TeX skips the space after a control word, and `{}` ends it.
            if chars.next_if_eq(&'{').is_some() {
                if chars.next() != Some('}') {
                    return Err(format!("an argument to `\\{name}`"));
                }
            } else {
                chars.next_if_eq(&' ');
            }
        }
        c => return Err(format!("the LaTeX escape `\\{c}`")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_umlauts_sharp_s_and_ampersand() {
        let raw = r#"M{\"u}ller \"Uber \"{o}l Stra\ss e Fu\ss{}ball {\&}"#;
        assert_eq!(text(raw).unwrap(), "Müller Über öl Straße Fußball &");
    }

    #[test]
    fn converts_dashes_tilde_and_braces() {
        assert_eq!(text("pp.~3--5 {NASA} --- a\n  b").unwrap(), "pp.\u{a0}3–5 NASA — a b");
    }

    #[test]
    fn rejects_unsupported_commands_without_leaking_backslashes() {
        assert_eq!(text(r"\textbf{x}").unwrap_err(), "the LaTeX command `\\textbf`");
        assert_eq!(text(r"\~n").unwrap_err(), "the LaTeX escape `\\~`");
        assert_eq!(text(r#"\"x"#).unwrap_err(), "the accent `\\\"x`");
    }

    #[test]
    fn verbatim_keeps_urls_intact() {
        assert_eq!(
            verbatim(r"{https://a.org/~x/a\_b?c=1\&d=2}"),
            "https://a.org/~x/a_b?c=1&d=2"
        );
    }
}
