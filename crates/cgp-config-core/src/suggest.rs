use alloc::vec::Vec;

/// Edit distance between `a` and `b`, counted in characters: insertions, deletions,
/// substitutions, and transpositions of two adjacent characters each cost 1 (optimal
/// string alignment distance).
///
/// ```
/// assert_eq!(cgp_config_core::edit_distance("seconds", "secnods"), 1);
/// assert_eq!(cgp_config_core::edit_distance("kitten", "sitting"), 3);
/// assert_eq!(cgp_config_core::edit_distance("port", "port"), 0);
/// ```
#[must_use]
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let width = b.len() + 1;
    // Three rolling rows: i-2, i-1, i.
    let mut before: Vec<usize> = alloc::vec![0; width];
    let mut previous: Vec<usize> = (0..width).collect();
    let mut current: Vec<usize> = alloc::vec![0; width];
    for i in 1..=a.len() {
        current[0] = i;
        for j in 1..width {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (previous[j - 1] + cost)
                .min(previous[j] + 1)
                .min(current[j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(before[j - 2] + 1);
            }
            current[j] = best;
        }
        core::mem::swap(&mut before, &mut previous);
        core::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The candidate closest to `input`, when it is close enough to be a likely typo.
///
/// "Close enough" is an edit distance of at most one third of the longer string
/// (minimum 1). Ties go to the first candidate. Used for "did you mean" hints on unknown
/// keys and enum variants.
///
/// ```
/// let fields = ["port", "host", "timeout"];
/// assert_eq!(cgp_config_core::suggest("prot", fields), Some("port"));
/// assert_eq!(cgp_config_core::suggest("database", fields), None);
/// ```
pub fn suggest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(&'a str, usize)> = None;
    for candidate in candidates {
        let distance = edit_distance(input, candidate);
        let longest = input.chars().count().max(candidate.chars().count());
        let allowed = (longest / 3).max(1);
        if distance == 0 || distance > allowed {
            continue;
        }
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((candidate, distance));
        }
    }
    best.map(|(candidate, _)| candidate)
}

#[cfg(test)]
mod tests {
    use super::{edit_distance, suggest};

    #[test]
    fn distances() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("über", "uber"), 1);
        assert_eq!(edit_distance("ab", "ba"), 1);
        assert_eq!(edit_distance("abc", "ca"), 3);
    }

    #[test]
    fn suggestions() {
        assert_eq!(suggest("hots", ["port", "host"]), Some("host"));
        assert_eq!(
            suggest("port", ["port"]),
            None,
            "an exact match is not a typo"
        );
        assert_eq!(suggest("x", core::iter::empty()), None);
        assert_eq!(suggest("ab", ["ac", "ad"]), Some("ac"));
    }
}
