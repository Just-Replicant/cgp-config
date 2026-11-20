//! Both locations of a duplicate TOML key.
//!
//! The `toml` crate rejects a duplicate key with the span of the second definition only
//! (its error drops the first). This module replays the document's parse events
//! (`toml_parser`, the lexer and event parser `toml` itself uses) and records the full
//! path and key span of every definition: key-value pairs (dotted keys included), table
//! headers, inline tables, and arrays of tables (with their index). The error's span
//! then names the second definition, and the earliest record with the same path is the
//! first.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use cgp_config_core::{KeyPath, PathSegment};
use toml_parser::decoder::Encoding;
use toml_parser::parser::EventReceiver;
use toml_parser::{ErrorSink, Raw, Source, Span};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Seg {
    Key(String),
    Index(usize),
}

enum Frame {
    Table(Vec<Seg>),
    Array { path: Vec<Seg>, index: usize },
}

/// One definition: its full path and the span of its last key.
struct Record {
    path: Vec<Seg>,
    key: Span,
}

struct Tracker<'s> {
    text: &'s str,
    header: Vec<Seg>,
    keys: Vec<(String, Span)>,
    stack: Vec<Frame>,
    pending: Option<Vec<Seg>>,
    array_tables: BTreeMap<Vec<Seg>, usize>,
    records: Vec<Record>,
}

impl Tracker<'_> {
    /// The header keys with the index of the current element of every array of tables
    /// they pass through (`[srv.tls]` after `[[srv]]` is `srv[n].tls`).
    fn header_path(&self) -> Vec<Seg> {
        let mut path = Vec::with_capacity(self.keys.len());
        for (key, _) in &self.keys {
            path.push(Seg::Key(key.clone()));
            if let Some(count) = self.array_tables.get(&path) {
                path.push(Seg::Index(count.saturating_sub(1)));
            }
        }
        path
    }

    fn prefix(&self) -> Vec<Seg> {
        match self.stack.last() {
            None => self.header.clone(),
            Some(Frame::Table(path)) => path.clone(),
            Some(Frame::Array { path, index }) => {
                let mut path = path.clone();
                path.push(Seg::Index(*index));
                path
            }
        }
    }

    /// The path of a value that starts now: after `key =`, or an array element.
    fn value_path(&mut self) -> Vec<Seg> {
        self.pending.take().unwrap_or_else(|| self.prefix())
    }
}

impl EventReceiver for Tracker<'_> {
    fn std_table_open(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        self.keys.clear();
        self.stack.clear();
    }

    fn std_table_close(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        let path = self.header_path();
        if let Some((_, span)) = self.keys.last() {
            self.records.push(Record {
                path: path.clone(),
                key: *span,
            });
        }
        self.header = path;
        self.keys.clear();
    }

    fn array_table_open(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        self.keys.clear();
        self.stack.clear();
    }

    fn array_table_close(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        // The last key names the array itself: resolve the prefix, then count.
        let last = self.keys.pop();
        let mut path = self.header_path();
        if let Some((key, _)) = last {
            path.push(Seg::Key(key));
            let count = self.array_tables.entry(path.clone()).or_insert(0);
            *count += 1;
            path.push(Seg::Index(*count - 1));
        }
        self.header = path;
        self.keys.clear();
    }

    fn inline_table_open(&mut self, _span: Span, _error: &mut dyn ErrorSink) -> bool {
        let path = self.value_path();
        self.stack.push(Frame::Table(path));
        true
    }

    fn inline_table_close(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        self.stack.pop();
    }

    fn array_open(&mut self, _span: Span, _error: &mut dyn ErrorSink) -> bool {
        let path = self.value_path();
        self.stack.push(Frame::Array { path, index: 0 });
        true
    }

    fn array_close(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        self.stack.pop();
    }

    fn simple_key(&mut self, span: Span, kind: Option<Encoding>, _error: &mut dyn ErrorSink) {
        let Some(raw) = self.text.get(span.start()..span.end()) else {
            return;
        };
        let mut key = String::new();
        Raw::new_unchecked(raw, kind, span).decode_key(&mut key, &mut ());
        self.keys.push((key, span));
    }

    fn key_val_sep(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        let mut path = self.prefix();
        path.extend(self.keys.iter().map(|(key, _)| Seg::Key(key.clone())));
        if let Some((_, span)) = self.keys.last() {
            self.records.push(Record {
                path: path.clone(),
                key: *span,
            });
        }
        self.pending = Some(path);
        self.keys.clear();
    }

    fn scalar(&mut self, _span: Span, _kind: Option<Encoding>, _error: &mut dyn ErrorSink) {
        self.pending = None;
    }

    fn value_sep(&mut self, _span: Span, _error: &mut dyn ErrorSink) {
        if let Some(Frame::Array { index, .. }) = self.stack.last_mut() {
            *index += 1;
        }
    }
}

/// The path of the duplicate whose (second) key starts at `second`, and the span of its
/// first definition, when the events allow both to be found.
pub(super) fn locate(text: &str, second: usize) -> Option<(KeyPath, Option<(usize, usize)>)> {
    let source = Source::new(text);
    let tokens = source.lex().into_vec();
    let mut tracker = Tracker {
        text,
        header: Vec::new(),
        keys: Vec::new(),
        stack: Vec::new(),
        pending: None,
        array_tables: BTreeMap::new(),
        records: Vec::new(),
    };
    toml_parser::parser::parse_document(&tokens, &mut tracker, &mut ());
    let records = tracker.records;
    let duplicate = records.iter().position(|record| {
        let (start, end) = (record.key.start(), record.key.end());
        start <= second && second < end.max(start + 1)
    })?;
    let path = &records[duplicate].path;
    let first = records[..duplicate]
        .iter()
        .find(|record| record.path == *path)
        // A table header after dotted keys that created the table implicitly.
        .or_else(|| {
            records[..duplicate]
                .iter()
                .find(|record| record.path.starts_with(path))
        })
        .map(|record| (record.key.start(), record.key.end()));
    let key_path = KeyPath::from_segments(path.iter().map(|segment| match segment {
        Seg::Key(key) => PathSegment::Key(key.as_str().into()),
        Seg::Index(index) => PathSegment::Index(*index),
    }));
    Some((key_path, first))
}
