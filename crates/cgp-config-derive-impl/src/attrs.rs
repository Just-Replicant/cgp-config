//! Raw attribute parsing: `#[config(...)]`, the `#[serde(...)]` keys that change field
//! keys or defaults, and doc comments. Checks across attributes live in `model`.

use proc_macro2::Span;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::{Attribute, Expr, ExprLit, Lit, LitStr, Meta, Token, UnOp};

use crate::case::RenameRule;

/// Collects errors so one expansion reports all of them.
#[derive(Default)]
pub(crate) struct Errors(Option<syn::Error>);

impl Errors {
    pub(crate) fn push(&mut self, error: syn::Error) {
        match &mut self.0 {
            Some(all) => all.combine(error),
            None => self.0 = Some(error),
        }
    }

    pub(crate) fn add(&mut self, span: Span, message: impl core::fmt::Display) {
        self.push(syn::Error::new(span, message));
    }

    pub(crate) fn finish<T>(self, value: T) -> syn::Result<T> {
        self.0.map_or(Ok(value), Err)
    }
}

/// A const default (`#[config(default = ...)]`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DefaultValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Array(Vec<DefaultValue>),
}

impl DefaultValue {
    pub(crate) fn from_expr(expr: &Expr) -> syn::Result<Self> {
        const HELP: &str =
            "a default must be a literal (bool, integer, float, string) or an array of literals";
        match expr {
            Expr::Lit(ExprLit { lit, .. }) => match lit {
                Lit::Bool(value) => Ok(Self::Bool(value.value)),
                Lit::Int(value) => Ok(Self::UInt(value.base10_parse()?)),
                Lit::Float(value) => finite(value),
                Lit::Str(value) => Ok(Self::Str(value.value())),
                other => Err(syn::Error::new(other.span(), HELP)),
            },
            Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => match &*unary.expr {
                Expr::Lit(ExprLit {
                    lit: Lit::Int(value),
                    ..
                }) => {
                    let magnitude: u64 = value.base10_parse()?;
                    let negative = i64::try_from(magnitude).map(|m| -m).or_else(|_| {
                        if magnitude == i64::MIN.unsigned_abs() {
                            Ok(i64::MIN)
                        } else {
                            Err(syn::Error::new(
                                value.span(),
                                "integer default out of range for i64",
                            ))
                        }
                    })?;
                    Ok(Self::Int(negative))
                }
                Expr::Lit(ExprLit {
                    lit: Lit::Float(value),
                    ..
                }) => match finite(value)? {
                    Self::Float(float) => Ok(Self::Float(-float)),
                    other => Ok(other),
                },
                _ => Err(syn::Error::new(expr.span(), HELP)),
            },
            Expr::Array(array) => array
                .elems
                .iter()
                .map(Self::from_expr)
                .collect::<syn::Result<Vec<_>>>()
                .map(Self::Array),
            Expr::Group(group) => Self::from_expr(&group.expr),
            Expr::Paren(paren) => Self::from_expr(&paren.expr),
            _ => Err(syn::Error::new(expr.span(), HELP)),
        }
    }
}

fn finite(lit: &syn::LitFloat) -> syn::Result<DefaultValue> {
    let value: f64 = lit.base10_parse()?;
    if value.is_finite() {
        Ok(DefaultValue::Float(value))
    } else {
        Err(syn::Error::new(
            lit.span(),
            "a float default must be finite",
        ))
    }
}

/// A value with the span to report it at.
#[derive(Debug, Clone)]
pub(crate) struct Spanned<T> {
    pub(crate) value: T,
    pub(crate) span: Span,
}

fn set<T>(
    slot: &mut Option<Spanned<T>>,
    meta: &ParseNestedMeta<'_>,
    name: &str,
    value: T,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate `{name}`")));
    }
    *slot = Some(Spanned {
        value,
        span: meta.path.span(),
    });
    Ok(())
}

fn flag(slot: &mut Option<Span>, meta: &ParseNestedMeta<'_>, name: &str) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        return Err(meta.error(format!("`{name}` takes no value")));
    }
    if slot.is_some() {
        return Err(meta.error(format!("duplicate `{name}`")));
    }
    *slot = Some(meta.path.span());
    Ok(())
}

fn string(meta: &ParseNestedMeta<'_>) -> syn::Result<LitStr> {
    meta.value()?.parse::<LitStr>()
}

fn ident_name(meta: &ParseNestedMeta<'_>) -> String {
    let path = &meta.path;
    path.get_ident().map_or_else(
        || quote::quote!(#path).to_string().replace(' ', ""),
        ToString::to_string,
    )
}

/// `#[config(...)]` on the struct.
#[derive(Default)]
pub(crate) struct ContainerAttrs {
    pub(crate) section: Option<Spanned<String>>,
    pub(crate) version: Option<Spanned<u32>>,
    pub(crate) env_prefix: Option<Spanned<String>>,
    pub(crate) deny_unknown: Option<Span>,
    pub(crate) rename_all: Option<Spanned<RenameRule>>,
    pub(crate) krate: Option<Spanned<syn::Path>>,
}

const CONTAINER_KEYS: &str =
    "section, version, env_prefix, deny_unknown, rename_all, crate (and `migrate`, from 0.2)";

impl ContainerAttrs {
    pub(crate) fn parse(attrs: &[Attribute], errors: &mut Errors) -> Self {
        let mut out = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("config")) {
            let result = attr.parse_nested_meta(|meta| {
                let name = ident_name(&meta);
                match name.as_str() {
                    "section" => {
                        let lit = string(&meta)?;
                        set(&mut out.section, &meta, "section", lit.value())
                    }
                    "version" => {
                        let lit = meta.value()?.parse::<syn::LitInt>()?;
                        let version: u32 = lit.base10_parse()?;
                        if version == 0 {
                            return Err(syn::Error::new(lit.span(), "versions start at 1"));
                        }
                        set(&mut out.version, &meta, "version", version)
                    }
                    "env_prefix" => {
                        let lit = string(&meta)?;
                        let value = lit.value();
                        if value.is_empty()
                            || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            return Err(syn::Error::new(
                                lit.span(),
                                "an env prefix is ASCII letters, digits, and `_`",
                            ));
                        }
                        set(&mut out.env_prefix, &meta, "env_prefix", value)
                    }
                    "deny_unknown" => flag(&mut out.deny_unknown, &meta, "deny_unknown"),
                    "rename_all" => {
                        let lit = string(&meta)?;
                        let rule = RenameRule::parse(&lit.value()).ok_or_else(|| {
                            syn::Error::new(
                                lit.span(),
                                format!(
                                    "unknown rename rule; expected one of {}",
                                    RenameRule::NAMES.join(", ")
                                ),
                            )
                        })?;
                        set(&mut out.rename_all, &meta, "rename_all", rule)
                    }
                    "crate" => {
                        let lit = string(&meta)?;
                        let path = lit.parse::<syn::Path>()?;
                        set(&mut out.krate, &meta, "crate", path)
                    }
                    "migrate" => {
                        // Consume the arguments so the message points at `migrate`.
                        if meta.input.peek(Token![=]) {
                            let _: Expr = meta.value()?.parse()?;
                        } else if meta.input.peek(syn::token::Paren) {
                            meta.parse_nested_meta(|inner| {
                                if inner.input.peek(Token![=]) {
                                    let _: Expr = inner.value()?.parse()?;
                                }
                                Ok(())
                            })?;
                        }
                        Err(meta.error(
                            "`migrate` is available from cgp-config 0.2 (schema migrations)",
                        ))
                    }
                    _ => Err(meta.error(format!(
                        "unknown `config` attribute `{name}`; expected one of: {CONTAINER_KEYS}"
                    ))),
                }
            });
            if let Err(error) = result {
                errors.push(error);
            }
        }
        out
    }
}

/// `#[config(...)]` on a field.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    pub(crate) default: Option<Spanned<DefaultValue>>,
    pub(crate) env: Option<Spanned<String>>,
    pub(crate) env_raw: Option<Spanned<String>>,
    pub(crate) secret: Option<Span>,
    pub(crate) aliases: Vec<Spanned<String>>,
    pub(crate) deprecated: Option<Spanned<String>>,
    pub(crate) reload: Option<Spanned<String>>,
    pub(crate) merge: Option<Spanned<String>>,
    pub(crate) interpolate: Option<Span>,
    pub(crate) parse: Option<Spanned<String>>,
    pub(crate) flatten: Option<Span>,
    pub(crate) nested: Option<Span>,
    pub(crate) mount: Option<Spanned<String>>,
    pub(crate) skip: Option<Span>,
}

const FIELD_KEYS: &str = "default, env, env_raw, secret, alias, deprecated, reload, merge, \
                          interpolate, parse, flatten, nested, mount, skip";

fn env_name(lit: &LitStr) -> syn::Result<String> {
    let value = lit.value();
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(syn::Error::new(
            lit.span(),
            "an env name is ASCII letters, digits, and `_`",
        ));
    }
    if value.contains("__") {
        return Err(syn::Error::new(
            lit.span(),
            "an env name must not contain the separator `__`",
        ));
    }
    Ok(value)
}

fn one_of(lit: &LitStr, allowed: &[&str], what: &str) -> syn::Result<String> {
    let value = lit.value();
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(syn::Error::new(
            lit.span(),
            format!(
                "unknown {what} `{value}`; expected one of: {}",
                allowed.join(", ")
            ),
        ))
    }
}

impl FieldAttrs {
    pub(crate) fn parse(attrs: &[Attribute], errors: &mut Errors) -> Self {
        let mut out = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("config")) {
            let result = attr.parse_nested_meta(|meta| {
                let name = ident_name(&meta);
                match name.as_str() {
                    "default" => {
                        if !meta.input.peek(Token![=]) {
                            return Err(meta.error(
                                "`default` needs a value (`default = 8080`); for `Default::default()` use `#[serde(default)]`",
                            ));
                        }
                        let expr: Expr = meta.value()?.parse()?;
                        let value = DefaultValue::from_expr(&expr)?;
                        set(&mut out.default, &meta, "default", value)
                    }
                    "env" => {
                        let value = env_name(&string(&meta)?)?;
                        set(&mut out.env, &meta, "env", value)
                    }
                    "env_raw" => {
                        let value = env_name(&string(&meta)?)?;
                        set(&mut out.env_raw, &meta, "env_raw", value)
                    }
                    "secret" => flag(&mut out.secret, &meta, "secret"),
                    "alias" => {
                        let lit = string(&meta)?;
                        out.aliases.push(Spanned {
                            value: lit.value(),
                            span: lit.span(),
                        });
                        Ok(())
                    }
                    "deprecated" => {
                        let note = if meta.input.peek(Token![=]) {
                            string(&meta)?.value()
                        } else {
                            "deprecated".to_owned()
                        };
                        set(&mut out.deprecated, &meta, "deprecated", note)
                    }
                    "reload" => {
                        let value = one_of(&string(&meta)?, &["live", "restart"], "reload policy")?;
                        set(&mut out.reload, &meta, "reload", value)
                    }
                    "merge" => {
                        let value = one_of(&string(&meta)?, &["deep", "replace", "append"], "merge policy")?;
                        set(&mut out.merge, &meta, "merge", value)
                    }
                    "interpolate" => flag(&mut out.interpolate, &meta, "interpolate"),
                    "parse" => {
                        let value = one_of(&string(&meta)?, &["duration", "bytes"], "parser")?;
                        set(&mut out.parse, &meta, "parse", value)
                    }
                    "flatten" => flag(&mut out.flatten, &meta, "flatten"),
                    "nested" => flag(&mut out.nested, &meta, "nested"),
                    "mount" => {
                        let lit = string(&meta)?;
                        let value = lit.value();
                        if value.is_empty() || value.contains('.') || value.starts_with('$') {
                            return Err(syn::Error::new(
                                lit.span(),
                                "a mount is one non-empty key without `.` that does not start with `$`",
                            ));
                        }
                        set(&mut out.mount, &meta, "mount", value)
                    }
                    "skip" => flag(&mut out.skip, &meta, "skip"),
                    _ => Err(meta.error(format!("unknown `config` attribute `{name}`; expected one of: {FIELD_KEYS}"))),
                }
            });
            if let Err(error) = result {
                errors.push(error);
            }
        }
        out
    }
}

/// The `#[serde(...)]` keys that matter on the struct.
#[derive(Default)]
pub(crate) struct SerdeContainer {
    pub(crate) rename_all: Option<Spanned<RenameRule>>,
    pub(crate) default: bool,
    pub(crate) deny_unknown_fields: bool,
}

/// The `#[serde(...)]` keys that matter on a field.
#[derive(Default)]
pub(crate) struct SerdeField {
    pub(crate) rename: Option<String>,
    pub(crate) default: bool,
    pub(crate) skip: bool,
    pub(crate) flatten: Option<Span>,
}

fn serde_metas(attrs: &[Attribute]) -> Vec<Meta> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("serde"))
        .filter_map(|attr| {
            attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .ok()
        })
        .flatten()
        .collect()
}

/// The string of `key = "value"`, or of `key(deserialize = "value")`.
fn serde_string(meta: &Meta) -> Option<String> {
    match meta {
        Meta::NameValue(pair) => match &pair.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(lit), ..
            }) => Some(lit.value()),
            _ => None,
        },
        Meta::List(list) => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .ok()?
            .iter()
            .find(|inner| inner.path().is_ident("deserialize"))
            .and_then(serde_string),
        Meta::Path(_) => None,
    }
}

impl SerdeContainer {
    pub(crate) fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for meta in serde_metas(attrs) {
            let path = meta.path();
            if path.is_ident("rename_all") {
                if let Some(rule) = serde_string(&meta).as_deref().and_then(RenameRule::parse) {
                    out.rename_all = Some(Spanned {
                        value: rule,
                        span: meta.span(),
                    });
                }
            } else if path.is_ident("default") {
                out.default = true;
            } else if path.is_ident("deny_unknown_fields") {
                out.deny_unknown_fields = true;
            }
        }
        out
    }
}

impl SerdeField {
    pub(crate) fn parse(attrs: &[Attribute]) -> Self {
        let mut out = Self::default();
        for meta in serde_metas(attrs) {
            let path = meta.path();
            if path.is_ident("rename") {
                out.rename = serde_string(&meta);
            } else if path.is_ident("default") {
                out.default = true;
            } else if path.is_ident("skip") || path.is_ident("skip_deserializing") {
                out.skip = true;
            } else if path.is_ident("flatten") {
                out.flatten = Some(meta.span());
            }
        }
        out
    }
}

/// The doc comment, lines trimmed and joined.
pub(crate) fn doc(attrs: &[Attribute]) -> String {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(pair) => match &pair.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(lit), ..
                }) => Some(lit.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|line| line.trim().to_owned())
        .collect();
    lines.join("\n").trim().to_owned()
}
