//! Which case the lyrics are drawn in.
//!
//! Every mapping here is one character to one character. The sweep and the
//! syllable reveal count characters of the source line, so a `ß` that became
//! `SS` would put every highlight after it one letter out. A character whose
//! other case is longer than itself keeps its own form instead.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Case {
    /// All capitals, the look the program has always had.
    #[default]
    Upper,
    /// All lowercase.
    Lower,
}

impl Case {
    pub fn apply(self, text: &str) -> String {
        match self {
            Case::Upper => text.chars().map(upper).collect(),
            Case::Lower => text.chars().map(lower).collect(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Case::Upper => "upper",
            Case::Lower => "lower",
        }
    }

    /// The other one, for the `c` key.
    pub fn next(self) -> Case {
        match self {
            Case::Upper => Case::Lower,
            Case::Lower => Case::Upper,
        }
    }
}

impl std::str::FromStr for Case {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "upper" => Ok(Case::Upper),
            "lower" => Ok(Case::Lower),
            _ => Err(format!("unknown case `{s}`; expected upper or lower")),
        }
    }
}

fn upper(c: char) -> char {
    // The capital eszett exists, and is what a sign painter would use.
    if c == 'ß' {
        return 'ẞ';
    }
    single(c.to_uppercase()).unwrap_or(c)
}

fn lower(c: char) -> char {
    single(c.to_lowercase()).unwrap_or(c)
}

fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED: &str = "Straße İstanbul ŉ ǅ Ǆ ﬁ Señor";

    #[test]
    fn every_case_keeps_the_character_count() {
        for case in [Case::Upper, Case::Lower] {
            assert_eq!(
                case.apply(MIXED).chars().count(),
                MIXED.chars().count(),
                "{case:?}"
            );
        }
    }

    #[test]
    fn the_cases_do_what_they_say() {
        assert_eq!(Case::Upper.apply("Señor ß"), "SEÑOR ẞ");
        assert_eq!(Case::Lower.apply("Señor ẞ"), "señor ß");
    }

    #[test]
    fn the_key_toggles_between_the_two() {
        assert_eq!(Case::Upper.next(), Case::Lower);
        assert_eq!(Case::Lower.next(), Case::Upper);
    }

    #[test]
    fn labels_parse_back() {
        for case in [Case::Upper, Case::Lower] {
            assert_eq!(case.label().parse::<Case>(), Ok(case));
        }
        assert!("original".parse::<Case>().is_err());
    }
}
