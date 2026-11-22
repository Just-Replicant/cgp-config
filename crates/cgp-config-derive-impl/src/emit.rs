//! Code generation from the checked [`Section`].

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned as _;

use crate::attrs::DefaultValue;
use crate::model::{Field, Section, Shape, StructShape};

pub(crate) fn expand(section: &Section) -> TokenStream {
    let krate = &section.krate;
    let config_section = config_section_impl(section);
    let validate = validate_impl(section);
    let getter = section.getter.then(|| getter_trait(section));
    let partial = section.partial.then(|| partial_items(section));
    quote! {
        #[allow(non_upper_case_globals, unused_qualifications, clippy::all, clippy::pedantic)]
        const _: () = {
            use #krate::__private as __cgp;
            #config_section
            #validate
        };
        #getter
        #partial
    }
}

fn default_tokens(value: &DefaultValue) -> TokenStream {
    match value {
        DefaultValue::Bool(value) => quote!(__cgp::ConstDefault::Bool(#value)),
        DefaultValue::Int(value) => {
            let lit = Literal::i64_unsuffixed(*value);
            quote!(__cgp::ConstDefault::Int(#lit))
        }
        DefaultValue::UInt(value) => {
            let lit = Literal::u64_unsuffixed(*value);
            quote!(__cgp::ConstDefault::UInt(#lit))
        }
        DefaultValue::Float(value) => {
            let lit = Literal::f64_unsuffixed(*value);
            quote!(__cgp::ConstDefault::Float(#lit))
        }
        DefaultValue::Str(value) => quote!(__cgp::ConstDefault::Str(#value)),
        DefaultValue::Array(values) => {
            let values = values.iter().map(default_tokens);
            quote!(__cgp::ConstDefault::Array(&[#(#values),*]))
        }
    }
}

fn field_meta(field: &Field) -> TokenStream {
    let name = &field.name;
    let key = &field.key;
    let mut out = quote!(__cgp::FieldMeta::new(#name, #key));
    if field.doc_key != field.key {
        let doc_key = &field.doc_key;
        out.extend(quote!(.with_doc_key(#doc_key)));
    }
    if !field.doc.is_empty() {
        let doc = &field.doc;
        out.extend(quote!(.with_doc(#doc)));
    }
    let ty = &field.ty_display;
    out.extend(quote!(.with_type(#ty)));
    if let Some(default) = &field.default {
        let default = default_tokens(default);
        out.extend(quote!(.with_default(#default)));
    }
    if let Some(env) = &field.env {
        out.extend(quote!(.with_env(#env)));
    }
    if let Some(env_raw) = &field.env_raw {
        out.extend(quote!(.with_env_raw(#env_raw)));
    }
    if !field.aliases.is_empty() {
        let aliases = &field.aliases;
        out.extend(quote!(.with_aliases(&[#(#aliases),*])));
    }
    if let Some(note) = &field.deprecated {
        out.extend(quote!(.with_deprecated(#note)));
    }
    if field.secret {
        out.extend(quote!(.with_secret(true)));
    }
    if field.restart {
        out.extend(quote!(.with_reload(__cgp::ReloadPolicy::Restart)));
    }
    if let Some(merge) = &field.merge {
        let variant = match merge.as_str() {
            "replace" => format_ident!("Replace"),
            "append" => format_ident!("Append"),
            _ => format_ident!("Deep"),
        };
        out.extend(quote!(.with_merge(__cgp::FieldMerge::#variant)));
    }
    if field.interpolate {
        out.extend(quote!(.with_interpolate(true)));
    }
    if let Some(parse) = &field.parse {
        let variant = if parse == "bytes" {
            format_ident!("Bytes")
        } else {
            format_ident!("Duration")
        };
        out.extend(quote!(.with_parse(__cgp::ParseKind::#variant)));
    }
    if field.flatten {
        out.extend(quote!(.with_flatten(true)));
    }
    if field.optional {
        out.extend(quote!(.with_optional(true)));
    }
    if let Some(nested) = &field.nested {
        let section = &nested.section;
        let shape = match nested.shape {
            Shape::Single => format_ident!("Single"),
            Shape::Seq => format_ident!("Seq"),
            Shape::Map => format_ident!("Map"),
        };
        let meta_fn = quote_spanned!(section.span()=> <#section as __cgp::ConfigSection>::meta);
        // A single section cannot contain its parent (the type would be infinite), so a
        // direct reference is safe and lets the compile-time key check walk into it.
        let section_ref = (nested.shape == Shape::Single).then(|| {
            let meta = quote_spanned!(section.span()=> <#section as __cgp::ConfigSection>::META);
            quote!(.with_section_ref(#meta))
        });
        out.extend(quote!(.with_nested(__cgp::NestedMeta::new(
            __cgp::NestedShape::#shape,
            #meta_fn,
        )#section_ref)));
    }
    if field.required {
        out.extend(quote!(.with_required(true)));
    }
    out
}

fn config_section_impl(section: &Section) -> TokenStream {
    let ident = &section.ident;
    let name = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = section.generics.split_for_impl();
    let mut where_clause = where_clause
        .cloned()
        .unwrap_or_else(|| syn::parse_quote!(where));
    for field in section.fields.iter().filter(|field| !field.skip) {
        if let Some(nested) = &field.nested {
            let ty = &nested.section;
            // Spanned at the field's type, so "not a section" points there.
            where_clause
                .predicates
                .push(syn::parse_quote_spanned!(ty.span()=> #ty: __cgp::ConfigSection));
        }
    }
    let fields = section
        .fields
        .iter()
        .filter(|field| !field.skip)
        .map(field_meta);
    let mut meta = quote!(__cgp::ConfigMeta::new(#name, &[#(#fields),*]));
    if let Some(key) = &section.section {
        meta.extend(quote!(.with_section(#key)));
    }
    if !section.doc.is_empty() {
        let doc = &section.doc;
        meta.extend(quote!(.with_doc(#doc)));
    }
    if let Some(version) = section.version {
        meta.extend(quote!(.with_version(#version)));
    }
    if let Some(prefix) = &section.env_prefix {
        meta.extend(quote!(.with_env_prefix(#prefix)));
    }
    if section.deny_unknown {
        meta.extend(quote!(.with_deny_unknown(true)));
    }
    // Non-generic sections are checked where they are declared; a generic section is
    // checked when an instantiation's `META` is first used (any load uses it).
    let force = section.generics.params.is_empty().then(|| {
        quote! {
            const _: &'static __cgp::ConfigMeta = <#ident as __cgp::ConfigSection>::META;
        }
    });
    quote! {
        #[automatically_derived]
        impl #impl_generics __cgp::ConfigSection for #ident #ty_generics #where_clause {
            const META: &'static __cgp::ConfigMeta = __cgp::ConfigMeta::checked(&#meta);
        }
        #force
    }
}

fn validate_impl(section: &Section) -> TokenStream {
    let ident = &section.ident;
    let context = format_ident!("__CgpContext");
    let mut generics = section.generics.clone();
    generics.params.push(syn::parse_quote!(#context));
    let (impl_generics, _, _) = generics.split_for_impl();
    let (_, ty_generics, where_clause) = section.generics.split_for_impl();
    let mut where_clause = where_clause
        .cloned()
        .unwrap_or_else(|| syn::parse_quote!(where));
    where_clause
        .predicates
        .push(syn::parse_quote!(#context: __cgp::HasErrorType));

    let mut steps = Vec::new();
    let mut needs_aggregate = false;
    for field in &section.fields {
        if field.skip {
            continue;
        }
        let Some(nested) = &field.nested else {
            continue;
        };
        needs_aggregate = true;
        let section_ty = &nested.section;
        where_clause
            .predicates
            .push(syn::parse_quote!(#context: __cgp::CanValidateConfig<#section_ty>));
        let member = &field.ident;
        let key = &field.doc_key;
        let step = match (nested.shape, field.flatten, field.optional) {
            (Shape::Single, true, false) => quote! {
                let __acc = match __cgp::CanValidateConfig::<#section_ty>::validate_config(__context, &self.#member) {
                    ::core::result::Result::Ok(()) => __acc,
                    ::core::result::Result::Err(error) => __cgp::accumulate(__acc, error),
                };
            },
            (Shape::Single, true, true) => quote! {
                let __acc = match self.#member.as_ref().map(|value| {
                    __cgp::CanValidateConfig::<#section_ty>::validate_config(__context, value)
                }) {
                    ::core::option::Option::Some(::core::result::Result::Err(error)) => __cgp::accumulate(__acc, error),
                    _ => __acc,
                };
            },
            (Shape::Single, false, false) => quote! {
                let __acc = __cgp::nested::single(__context, #key, &self.#member, __acc);
            },
            (Shape::Single, false, true) => quote! {
                let __acc = __cgp::nested::option(__context, #key, self.#member.as_ref(), __acc);
            },
            (Shape::Seq, _, _) => quote! {
                let __acc = __cgp::nested::seq(__context, #key, self.#member.iter(), __acc);
            },
            (Shape::Map, _, _) => quote! {
                let __acc = __cgp::nested::map(__context, #key, self.#member.iter(), __acc);
            },
        };
        steps.push(step);
    }
    if needs_aggregate {
        where_clause.predicates.push(syn::parse_quote!(
            <#context as __cgp::HasErrorType>::Error: __cgp::AggregateError
        ));
    }
    let body = if steps.is_empty() {
        quote! {
            let _ = __context;
            ::core::result::Result::Ok(())
        }
    } else {
        quote! {
            let __acc = ::core::option::Option::None;
            #(#steps)*
            __cgp::nested::finish(__acc)
        }
    };
    quote! {
        #[automatically_derived]
        impl #impl_generics __cgp::ValidateSections<#context> for #ident #ty_generics #where_clause {
            fn validate_sections(
                &self,
                __context: &#context,
            ) -> ::core::result::Result<(), <#context as __cgp::HasErrorType>::Error> {
                #body
            }
        }
    }
}

/// `Has{Stem}Config` and `fn {stem}_config`. `HttpConfig` and `Http` both become
/// `HasHttpConfig` / `http_config`.
fn getter_idents(ident: &syn::Ident) -> (syn::Ident, syn::Ident) {
    let name = ident.to_string();
    let stem = name
        .strip_suffix("Config")
        .filter(|stem| {
            !stem.is_empty()
                && stem
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase())
        })
        .unwrap_or(name.as_str());
    let trait_ident = format_ident!("Has{stem}Config", span = ident.span());
    let method = format_ident!("{}", snake(&format!("{stem}Config")), span = ident.span());
    (trait_ident, method)
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn getter_trait(section: &Section) -> TokenStream {
    let vis = &section.vis;
    let ident = &section.ident;
    let generics = &section.generics;
    let (_, ty_generics, _) = generics.split_for_impl();
    let (trait_ident, method) = getter_idents(ident);
    let struct_name = ident.to_string();
    let doc = format!(
        "Borrows a [`{struct_name}`] from a context that owns it for the borrow.\n\n\
         Generated by `#[config(getter)]`. `UseField` reads a field of this type. \
         `WithProvider<ProjectSection<Path!(@...)>>` reads it from a nested field \
         (`Path!` takes a leading `@`)."
    );
    let method_doc = format!("The `{struct_name}` this context holds.");
    quote! {
        #[doc = #doc]
        #[::cgp_fork::prelude::cgp_getter]
        #vis trait #trait_ident #generics {
            #[doc = #method_doc]
            fn #method(&self) -> &#ident #ty_generics;
        }
    }
}

fn partial_items(section: &Section) -> TokenStream {
    let vis = &section.vis;
    let ident = &section.ident;
    let partial = format_ident!("{}Partial", ident);
    let krate = &section.krate;
    let (impl_generics, ty_generics, where_clause) = partial_impl_generics(section);
    let struct_generics = &section.generics;
    let struct_name = ident.to_string();
    let doc = format!(
        "Partial [`{struct_name}`]. Unset fields use `#[config(default)]`, or are reported together.\n\n\
         Generated by `#[config(partial)]`. `FinalizeOptionalAll::finalize_optional_all` \
         builds [`{struct_name}`]."
    );

    let mut defs = Vec::new();
    let mut inits = Vec::new();
    let mut marks = Vec::new();
    let mut tracked = Vec::new();
    let mut configurable = false;
    for field in &section.fields {
        if field.skip {
            let member = &field.ident;
            let ty = &field.ty;
            inits.push(quote!(#member: <#ty as ::core::default::Default>::default()));
            continue;
        }
        configurable = true;
        let member = &field.ident;
        let ty = partial_field_ty(field);
        let field_doc = if field.doc.is_empty() {
            format!("Optional `{}`.", field.name)
        } else {
            field.doc.clone()
        };
        defs.push(quote! {
            #[doc = #field_doc]
            pub #member: #ty,
        });
        if must_be_present(field) {
            let name = field.name.as_str();
            tracked.push(name);
            marks.push(quote! {
                if self.#member.is_none() {
                    missing.push(#name);
                }
            });
        }
        inits.push(field_init(field, krate));
    }

    let derive = configurable.then(|| quote!(#[derive(::cgp_fork::prelude::CgpData)]));
    let struct_item = match section.shape {
        StructShape::Unit => quote! {
            #[doc = #doc]
            #[allow(clippy::all, clippy::pedantic)]
            #vis struct #partial #struct_generics;
        },
        StructShape::Named => quote! {
            #[doc = #doc]
            #[allow(clippy::all, clippy::pedantic)]
            #derive
            #vis struct #partial #struct_generics {
                #(#defs)*
            }
        },
    };
    let value = match section.shape {
        StructShape::Unit => quote!(#ident),
        StructShape::Named => quote!(#ident #ty_generics { #(#inits),* }),
    };
    quote! {
        #struct_item

        #[automatically_derived]
        #[allow(clippy::all, clippy::pedantic, missing_docs)]
        impl #impl_generics #krate::__private::FinalizeOptionalAll for #partial #ty_generics #where_clause {
            type Output = #ident #ty_generics;

            fn finalize_optional_all(
                self,
            ) -> ::core::result::Result<Self::Output, #krate::__private::MissingFields> {
                let mut missing = #krate::__private::MissingFields::new(&[#(#tracked),*]);
                #(#marks)*
                if let ::core::result::Result::Err(error) = missing.into_result() {
                    return ::core::result::Result::Err(error);
                }
                ::core::result::Result::Ok(#value)
            }
        }
    }
}

fn partial_impl_generics(section: &Section) -> (TokenStream, TokenStream, TokenStream) {
    let mut generics = section.generics.clone();
    let skipped: Vec<_> = section
        .fields
        .iter()
        .filter(|field| field.skip)
        .map(|field| field.ty.clone())
        .collect();
    if !skipped.is_empty() {
        let where_clause = generics.make_where_clause();
        for ty in &skipped {
            where_clause
                .predicates
                .push(syn::parse_quote!(#ty: ::core::default::Default));
        }
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    (
        quote!(#impl_generics),
        quote!(#ty_generics),
        quote!(#where_clause),
    )
}

fn partial_field_ty(field: &Field) -> TokenStream {
    let ty = &field.ty;
    if field.optional {
        quote!(#ty)
    } else {
        quote!(::core::option::Option<#ty>)
    }
}

/// A required field with neither a const default nor `#[serde(default)]`.
fn must_be_present(field: &Field) -> bool {
    !field.optional && field.default.is_none() && !field.serde_default
}

fn field_init(field: &Field, krate: &syn::Path) -> TokenStream {
    let member = &field.ident;
    if field.optional {
        return quote!(#member: self.#member);
    }
    if let Some(fallback) = fallback_expr(field, krate) {
        return quote!(#member: self.#member.unwrap_or_else(|| #fallback));
    }
    if field.serde_default {
        let ty = &field.ty;
        return quote!(#member: self.#member.unwrap_or_else(<#ty as ::core::default::Default>::default));
    }
    quote!(#member: self.#member.unwrap())
}

fn fallback_expr(field: &Field, krate: &syn::Path) -> Option<TokenStream> {
    let value = field.default.as_ref()?;
    let ty = &field.ty;
    match (field.parse.as_deref(), value) {
        (Some("duration"), DefaultValue::Str(text)) => Some(quote! {
            #krate::__private::parse_duration(#text).expect("duration default")
        }),
        (Some("bytes"), DefaultValue::Str(text)) => Some(quote! {
            #krate::__private::parse_bytes(#text).expect("byte-size default")
        }),
        (Some("bytes"), DefaultValue::UInt(number)) => {
            let lit = Literal::u64_unsuffixed(*number);
            Some(quote!(#lit))
        }
        (_, DefaultValue::Str(text)) => Some(quote! {
            <#ty as ::core::convert::From<&str>>::from(#text)
        }),
        (_, DefaultValue::Bool(value)) => Some(quote!(#value)),
        (_, DefaultValue::Int(value)) => {
            let lit = Literal::i64_unsuffixed(*value);
            Some(quote!(#lit))
        }
        (_, DefaultValue::UInt(value)) => {
            let lit = Literal::u64_unsuffixed(*value);
            Some(quote!(#lit))
        }
        (_, DefaultValue::Float(value)) => {
            let lit = Literal::f64_unsuffixed(*value);
            Some(quote!(#lit))
        }
        (_, DefaultValue::Array(items)) => {
            let elems = items
                .iter()
                .map(array_element)
                .collect::<Option<Vec<_>>>()?;
            Some(quote!([#(#elems),*]))
        }
    }
}

fn array_element(value: &DefaultValue) -> Option<TokenStream> {
    match value {
        DefaultValue::Bool(value) => Some(quote!(#value)),
        DefaultValue::Int(value) => {
            let lit = Literal::i64_unsuffixed(*value);
            Some(quote!(#lit))
        }
        DefaultValue::UInt(value) => {
            let lit = Literal::u64_unsuffixed(*value);
            Some(quote!(#lit))
        }
        DefaultValue::Float(value) => {
            let lit = Literal::f64_unsuffixed(*value);
            Some(quote!(#lit))
        }
        DefaultValue::Str(text) => Some(quote!(#text)),
        DefaultValue::Array(_) => None,
    }
}
