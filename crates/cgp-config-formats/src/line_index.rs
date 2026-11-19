//! [`LineIndex`]: byte offsets to `line:column` in `O(log n)`.

use alloc::vec::Vec;

use cgp_config_core::Span;

/// The start offset of every line of a text, so spans of a large document cost a binary
/// search each instead of a rescan from the start.
#[derive(Debug, Clone)]
pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    /// Indexes `text`.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        Self { text, starts }
    }

    /// The span of `start..end`, with 1-based line and character column. Agrees with
    /// `Span::from_offsets`.
    ///
    /// ```
    /// use cgp_config_core::Span;
    /// use cgp_config_formats::LineIndex;
    ///
    /// let text = "a = 1\nü = \"x\"\n";
    /// let index = LineIndex::new(text);
    /// assert_eq!(index.span(11, 14), Span::from_offsets(text, 11, 14));
    /// assert_eq!((index.span(11, 14).line, index.span(11, 14).column), (2, 5));
    /// ```
    #[must_use]
    pub fn span(&self, start: usize, end: usize) -> Span {
        let mut start = start.min(self.text.len());
        while !self.text.is_char_boundary(start) {
            start -= 1;
        }
        let line = self
            .starts
            .partition_point(|&line_start| line_start <= start);
        let line_start = self.starts[line - 1];
        let column = self.text[line_start..start].chars().count() + 1;
        Span::new(
            start,
            end.max(start),
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        )
    }

    /// The indexed text.
    #[must_use]
    pub fn text(&self) -> &'a str {
        self.text
    }
}

#[cfg(test)]
mod tests {
    use cgp_config_core::Span;

    use super::LineIndex;

    #[test]
    fn agrees_with_from_offsets_everywhere() {
        let text = "a\n\nbc é\r\nxyz\n";
        let index = LineIndex::new(text);
        for start in 0..=text.len() + 2 {
            for end in [start, start + 1, start + 3] {
                assert_eq!(
                    index.span(start, end),
                    Span::from_offsets(text, start, end),
                    "{start}"
                );
            }
        }
        assert_eq!(index.text(), text);
    }
}
