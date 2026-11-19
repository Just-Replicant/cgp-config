//! serde's `rename_all` rules, applied to field names.

/// A `rename_all` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    /// Every accepted spelling, for error messages.
    pub(crate) const NAMES: &'static [&'static str] = &[
        "lowercase",
        "UPPERCASE",
        "PascalCase",
        "camelCase",
        "snake_case",
        "SCREAMING_SNAKE_CASE",
        "kebab-case",
        "SCREAMING-KEBAB-CASE",
    ];

    pub(crate) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            _ => return None,
        })
    }

    /// The key of a snake_case field name under this rule (serde's semantics).
    pub(crate) fn apply(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => pascal(field),
            Self::Camel => {
                let pascal = pascal(field);
                let mut chars = pascal.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_ascii_lowercase().to_string() + chars.as_str()
                })
            }
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => field.to_ascii_uppercase().replace('_', "-"),
        }
    }
}

fn pascal(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut upper = true;
    for c in field.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::RenameRule;

    #[test]
    fn rules_match_serde() {
        let cases = [
            ("lowercase", "max_conn"),
            ("UPPERCASE", "MAX_CONN"),
            ("PascalCase", "MaxConn"),
            ("camelCase", "maxConn"),
            ("snake_case", "max_conn"),
            ("SCREAMING_SNAKE_CASE", "MAX_CONN"),
            ("kebab-case", "max-conn"),
            ("SCREAMING-KEBAB-CASE", "MAX-CONN"),
        ];
        for (rule, expected) in cases {
            assert_eq!(
                RenameRule::parse(rule).unwrap().apply("max_conn"),
                expected,
                "{rule}"
            );
        }
        assert!(RenameRule::parse("Title Case").is_none());
        assert_eq!(RenameRule::NAMES.len(), cases.len());
    }
}
