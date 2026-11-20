//! The checked model of a section: attributes resolved against serde's and each other.

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Generics, Ident, Type};

use crate::attrs::{
    ContainerAttrs, DefaultValue, Errors, FieldAttrs, SerdeContainer, SerdeField, doc,
};
use crate::ty;

/// How a nested section is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Single,
    Seq,
    Map,
}

/// A nested section field: its shape and the section type.
pub(crate) struct NestedField {
    pub(crate) shape: Shape,
    pub(crate) section: Type,
}

/// One field of the section, as `FieldMeta` will describe it.
pub(crate) struct Field {
    pub(crate) ident: Ident,
    pub(crate) name: String,
    pub(crate) key: String,
    pub(crate) doc_key: String,
    pub(crate) doc: String,
    pub(crate) ty_display: String,
    pub(crate) default: Option<DefaultValue>,
    pub(crate) env: Option<String>,
    pub(crate) env_raw: Option<String>,
    pub(crate) aliases: Vec<String>,
    pub(crate) deprecated: Option<String>,
    pub(crate) secret: bool,
    pub(crate) restart: bool,
    pub(crate) merge: Option<String>,
    pub(crate) interpolate: bool,
    pub(crate) parse: Option<String>,
    pub(crate) flatten: bool,
    pub(crate) optional: bool,
    pub(crate) nested: Option<NestedField>,
    pub(crate) required: bool,
}

/// The whole section.
pub(crate) struct Section {
    pub(crate) ident: Ident,
    pub(crate) generics: Generics,
    pub(crate) krate: syn::Path,
    pub(crate) section: Option<String>,
    pub(crate) version: Option<u32>,
    pub(crate) env_prefix: Option<String>,
    pub(crate) deny_unknown: bool,
    pub(crate) doc: String,
    pub(crate) fields: Vec<Field>,
}

impl Section {
    pub(crate) fn from_input(input: &DeriveInput) -> syn::Result<Self> {
        let mut errors = Errors::default();
        let named = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(fields) => Some(&fields.named),
                Fields::Unit => None,
                Fields::Unnamed(fields) => {
                    errors.add(
                        fields.span(),
                        "ConfigSection needs named fields (a tuple struct has no keys)",
                    );
                    None
                }
            },
            Data::Enum(data) => {
                errors.add(
                    data.enum_token.span(),
                    "ConfigSection is derived for structs with named fields; an enum is a value, not a section",
                );
                None
            }
            Data::Union(data) => {
                errors.add(
                    data.union_token.span(),
                    "ConfigSection cannot be derived for a union",
                );
                None
            }
        };

        let container = ContainerAttrs::parse(&input.attrs, &mut errors);
        let serde = SerdeContainer::parse(&input.attrs);
        let rename_all = match (&container.rename_all, &serde.rename_all) {
            (Some(ours), Some(theirs)) if ours.value != theirs.value => {
                errors.add(
                    ours.span,
                    "`config(rename_all)` disagrees with `serde(rename_all)`; keep one (serde's is read automatically)",
                );
                Some(ours.value)
            }
            (Some(ours), _) => Some(ours.value),
            (None, theirs) => theirs.as_ref().map(|rule| rule.value),
        };
        if let Some(section) = &container.section
            && (section.value.is_empty()
                || section.value.starts_with('$')
                || section.value.contains('.'))
        {
            errors.add(
                section.span,
                "a section key is one non-empty key without `.` that does not start with `$`",
            );
        }

        let mut fields = Vec::new();
        for field in named.into_iter().flatten() {
            let Some(ident) = field.ident.clone() else {
                continue;
            };
            let attrs = FieldAttrs::parse(&field.attrs, &mut errors);
            let serde_field = SerdeField::parse(&field.attrs);
            if serde_field.skip {
                if let Some(span) = any_config_attr(&attrs, true) {
                    errors.add(
                        span,
                        "this field is skipped by serde, so `config` attributes have no effect",
                    );
                }
                continue;
            }
            if let Some(span) = attrs.skip {
                errors.add(
                    span,
                    "`config(skip)` needs `#[serde(skip)]` (or `skip_deserializing`) too, so both agree the field is not read",
                );
                continue;
            }
            fields.push(Self::field(
                ident,
                field,
                attrs,
                &serde_field,
                &serde,
                rename_all,
                &mut errors,
            ));
        }
        check_unique(&fields, &mut errors);

        let krate = container
            .krate
            .map_or_else(|| syn::parse_quote!(::cgp_config), |krate| krate.value);
        errors.finish(Self {
            ident: input.ident.clone(),
            generics: input.generics.clone(),
            krate,
            section: container.section.map(|s| s.value),
            version: container.version.map(|v| v.value),
            env_prefix: container.env_prefix.map(|p| p.value),
            deny_unknown: container.deny_unknown.is_some() || serde.deny_unknown_fields,
            doc: doc(&input.attrs),
            fields,
        })
    }

    fn field(
        ident: Ident,
        field: &syn::Field,
        attrs: FieldAttrs,
        serde_field: &SerdeField,
        serde: &SerdeContainer,
        rename_all: Option<crate::case::RenameRule>,
        errors: &mut Errors,
    ) -> Field {
        let name = ident.to_string();
        let name = name.strip_prefix("r#").unwrap_or(&name).to_owned();
        let key = serde_field
            .rename
            .clone()
            .unwrap_or_else(|| rename_all.map_or_else(|| name.clone(), |rule| rule.apply(&name)));
        if key.starts_with('$') {
            errors.add(
                ident.span(),
                format!(
                    "`{key}`: keys starting with `$` are reserved ($version, $profiles, $include)"
                ),
            );
        }
        let doc_key = attrs
            .mount
            .as_ref()
            .map_or_else(|| key.clone(), |mount| mount.value.clone());

        let option_inner = ty::option_inner(&field.ty);
        let optional = option_inner.is_some();
        let flatten = attrs.flatten.is_some();
        let nested = (attrs.nested.is_some() || flatten).then(|| {
            let base = option_inner.unwrap_or(&field.ty);
            if let Some(section) = ty::seq_inner(base).filter(|_| !optional) {
                NestedField {
                    shape: Shape::Seq,
                    section: section.clone(),
                }
            } else if let Some(section) = ty::map_value(base).filter(|_| !optional) {
                NestedField {
                    shape: Shape::Map,
                    section: section.clone(),
                }
            } else {
                NestedField {
                    shape: Shape::Single,
                    section: base.clone(),
                }
            }
        });

        if let (Some(span), true) = (serde_field.flatten, flatten) {
            errors.add(span, "use `#[config(flatten)]` alone: cgp-config moves the keys itself, so `serde(flatten)` must not be set");
        }
        if let (Some(span), Some(nested)) = (attrs.flatten, &nested)
            && nested.shape != Shape::Single
        {
            errors.add(
                span,
                "only a single section can be flattened (not a list or a map)",
            );
        }
        if nested.is_some() {
            let conflict = [
                (
                    attrs.default.as_ref().map(|d| d.span),
                    "default",
                    "a nested section takes defaults from its own fields",
                ),
                (
                    attrs.parse.as_ref().map(|p| p.span),
                    "parse",
                    "a nested section is not a text value",
                ),
                (
                    attrs.env.as_ref().map(|e| e.span),
                    "env",
                    "env names belong to the section's own fields",
                ),
                (
                    attrs.env_raw.as_ref().map(|e| e.span),
                    "env_raw",
                    "env names belong to the section's own fields",
                ),
                (
                    attrs.interpolate,
                    "interpolate",
                    "mark the strings inside the section instead",
                ),
            ];
            for (span, attr, why) in conflict {
                if let Some(span) = span {
                    errors.add(
                        span,
                        format!("`{attr}` cannot be used on a nested section: {why}"),
                    );
                }
            }
        }
        if flatten {
            if let Some(mount) = &attrs.mount {
                errors.add(
                    mount.span,
                    "a flattened section has no key of its own to mount",
                );
            }
            if let Some(alias) = attrs.aliases.first() {
                errors.add(
                    alias.span,
                    "a flattened section has no key of its own to alias",
                );
            }
        }
        if let (Some(parse), Some(default)) = (&attrs.parse, &attrs.default) {
            let fits = matches!(
                (parse.value.as_str(), &default.value),
                (_, DefaultValue::Str(_)) | ("bytes", DefaultValue::UInt(_))
            );
            if !fits {
                let expected = if parse.value == "duration" {
                    "a duration default is a string with a unit (`default = \"30s\"`)"
                } else {
                    "a byte-size default is a string (`\"10MiB\"`) or a whole number of bytes"
                };
                errors.add(default.span, expected);
            }
        }
        for alias in &attrs.aliases {
            if alias.value.is_empty() || alias.value.starts_with('$') || alias.value.contains('.') {
                errors.add(
                    alias.span,
                    "an alias is one non-empty key without `.` that does not start with `$`",
                );
            }
        }

        let has_default = attrs.default.is_some() || serde_field.default || serde.default;
        let required = !optional && !has_default && nested.is_none();
        Field {
            name,
            key,
            doc_key,
            doc: doc(&field.attrs),
            ty_display: ty::display(&field.ty),
            default: attrs.default.map(|d| d.value),
            env: attrs.env.map(|e| e.value),
            env_raw: attrs.env_raw.map(|e| e.value),
            aliases: attrs.aliases.into_iter().map(|a| a.value).collect(),
            deprecated: attrs.deprecated.map(|d| d.value),
            secret: attrs.secret.is_some(),
            restart: attrs.reload.is_some_and(|r| r.value == "restart"),
            merge: attrs.merge.map(|m| m.value),
            interpolate: attrs.interpolate.is_some(),
            parse: attrs.parse.map(|p| p.value),
            flatten,
            optional,
            nested,
            required,
            ident,
        }
    }
}

/// The span of any `config` attribute set on the field (`skip` included or not).
fn any_config_attr(attrs: &FieldAttrs, include_skip: bool) -> Option<Span> {
    let spans = [
        attrs.default.as_ref().map(|v| v.span),
        attrs.env.as_ref().map(|v| v.span),
        attrs.env_raw.as_ref().map(|v| v.span),
        attrs.secret,
        attrs.aliases.first().map(|v| v.span),
        attrs.deprecated.as_ref().map(|v| v.span),
        attrs.reload.as_ref().map(|v| v.span),
        attrs.merge.as_ref().map(|v| v.span),
        attrs.interpolate,
        attrs.parse.as_ref().map(|v| v.span),
        attrs.flatten,
        attrs.nested,
        attrs.mount.as_ref().map(|v| v.span),
        attrs.skip.filter(|_| !include_skip),
    ];
    spans.into_iter().flatten().next()
}

/// Document keys, aliases, and env names must not collide within a section. This gives
/// the best spans for clashes inside one struct; clashes that involve flattened sections
/// or `env_raw` names of nested sections are found by `ConfigMeta::checked`, which the
/// generated `META` calls at compile time.
fn check_unique(fields: &[Field], errors: &mut Errors) {
    let mut keys: Vec<(&str, &Ident)> = Vec::new();
    let mut env_names: Vec<(&str, &str, &Ident)> = Vec::new();
    for field in fields.iter().filter(|field| !field.flatten) {
        for key in
            core::iter::once(field.doc_key.as_str()).chain(field.aliases.iter().map(String::as_str))
        {
            if let Some((_, first)) = keys.iter().find(|(seen, _)| seen.eq_ignore_ascii_case(key)) {
                errors.add(
                    field.ident.span(),
                    format!("key `{key}` is also used by `{first}` (keys and aliases must differ, ignoring ASCII case, so env names stay unambiguous)"),
                );
            } else {
                keys.push((key, &field.ident));
            }
        }
        // `env` names a segment under the section; `env_raw` a process-global variable.
        let names = field
            .env
            .iter()
            .map(|name| ("env", name.as_str()))
            .chain(field.env_raw.iter().map(|name| ("env_raw", name.as_str())));
        for (kind, name) in names {
            let seen = env_names
                .iter()
                .find(|(seen_kind, seen, _)| *seen_kind == kind && seen.eq_ignore_ascii_case(name));
            if let Some((_, _, first)) = seen {
                errors.add(
                    field.ident.span(),
                    format!("`{kind}` name `{name}` is also used by `{first}`"),
                );
            } else {
                env_names.push((kind, name, &field.ident));
            }
        }
    }
}
