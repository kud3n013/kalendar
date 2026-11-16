//! The words the app shows, made translatable.
//!
//! Every text a person reads goes through [`tr`], or [`trf`] when values are
//! filled into it. The English text is written where it is used and is
//! itself the key, as gettext does it: the code stays readable, and a
//! translation is a table from those English texts to another language.
//!
//! To add a language: put a `CATALOG` of `(english, translated)` pairs in a
//! file under `src/strings/` and list it in [`CATALOGS`]. Texts a catalog
//! leaves out are shown in English. In a translation `{}` places the values
//! in the order the English gives them, and `{0}`, `{1}` … pick one, so a
//! sentence can put them in another order. The tests check that every pair
//! still matches a text in the code and keeps its places for values.
//!
//! Not translated: names people gave (calendars, lists, tasks), protocol
//! words, key names, and the messages of the command-line test aids.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

/// A translation: English text, then the same in another language.
pub type Catalog = &'static [(&'static str, &'static str)];

/// The translations there are, by language code (`de`, `pt_BR`, …).
const CATALOGS: &[(&str, Catalog)] = &[];

/// The language asked for: `KALENDAR_LANG`, else the system's
/// (`LC_ALL`, `LC_MESSAGES`, `LANG`), as in `de_DE.UTF-8`.
fn wanted() -> Option<String> {
    ["KALENDAR_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .map(|v| v.split(['.', '@']).next().unwrap_or("").to_string())
}

/// The catalog for `lang`: an exact match (`pt_BR`), else the language
/// alone (`pt`).
fn pick(lang: &str, catalogs: &[(&str, Catalog)]) -> Option<Catalog> {
    let base = lang.split('_').next().unwrap_or(lang);
    catalogs.iter().find(|c| c.0 == lang).or_else(|| catalogs.iter().find(|c| c.0 == base)).map(|c| c.1)
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let catalog = wanted().and_then(|l| pick(&l, CATALOGS)).unwrap_or(&[]);
        catalog.iter().copied().collect()
    })
}

/// `text` in the language in use.
pub fn tr(text: &'static str) -> &'static str {
    table().get(text).copied().unwrap_or(text)
}

/// Marks `text` for translation where it cannot be translated yet, as in a
/// `const` table; [`tr`] is called on it where it is shown.
pub const fn later(text: &'static str) -> &'static str {
    text
}

/// `text` in the language in use, with `values` put in its `{}` places.
pub fn trf(text: &'static str, values: &[&dyn Display]) -> String {
    fill(tr(text), values)
}

/// Puts `values` into `template`: `{}` takes the next one, `{n}` the nth.
/// `{{` and `}}` stand for the braces themselves.
fn fill(template: &str, values: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let (mut next, mut rest) = (0, template);
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        let close = match (tail.starts_with('{'), tail.find('}')) {
            (true, Some(c)) => c,
            _ => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
                continue;
            }
        };
        let inside = &tail[1..close];
        let n = if inside.is_empty() {
            next += 1;
            Some(next - 1)
        } else {
            inside.parse::<usize>().ok()
        };
        match n.and_then(|n| values.get(n)) {
            Some(v) => out.push_str(&v.to_string()),
            None => out.push_str(&tail[..=close]),
        }
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_fill_in_order_or_by_number() {
        assert_eq!(fill("Moved to {} at {}", &[&"Mon", &9]), "Moved to Mon at 9");
        assert_eq!(fill("um {1} am {0}", &[&"Mon", &9]), "um 9 am Mon");
        assert_eq!(fill("{{literal}} {}", &[&1]), "{literal} 1");
        assert_eq!(fill("missing {} {}", &[&1]), "missing 1 {}");
    }

    #[test]
    fn a_language_falls_back_to_its_base() {
        const DE: Catalog = &[("Undone", "Rückgängig gemacht")];
        let cats: &[(&str, Catalog)] = &[("de", DE)];
        assert!(pick("de_AT", cats).is_some());
        assert!(pick("fr_FR", cats).is_none());
    }

    #[test]
    fn english_is_shown_when_no_translation_is_found() {
        assert_eq!(tr("Some text no catalog has"), "Some text no catalog has");
    }

    /// Every text a catalog translates is still in the code, as a `tr` or
    /// `trf` text, and keeps the number of places for values.
    #[test]
    fn catalogs_match_the_code() {
        let mut code = String::new();
        let mut dirs = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    code.push_str(&std::fs::read_to_string(p).unwrap());
                }
            }
        }
        let places = |s: &str| fill(s, &[]).matches('{').count();
        for (lang, cat) in CATALOGS {
            for (en, other) in cat.iter() {
                let quoted = format!("{en:?}");
                assert!(["tr(", "trf(", "later("].iter().any(|f| code.contains(&format!("{f}{quoted}"))), "{lang}: {en:?} is no longer in the code");
                assert_eq!(places(en), places(other), "{lang}: {en:?} and its translation have different places for values");
            }
        }
    }
}
