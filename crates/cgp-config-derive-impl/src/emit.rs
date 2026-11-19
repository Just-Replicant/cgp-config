//! Code generation from the checked [`Section`].

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned as _;

use crate::attrs::DefaultValue;
use crate::model::{Field, Section, Shape};

pub(crate) fn expand(section: &Section) -> TokenStream {
    let krate = &section.krate;
    let config_section = config_section_impl(section);
    let validate = validate_impl(section);
    quote! {
        #[allow(non_upper_case_globals, unused_qualifications, clippy::all, clippy::pedantic)]
        const _: () = {
            use #krate::__private as __cgp;
            #config_section
            #validate
        };
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
        out.extend(quote!(.with_nested(__cgp::NestedMeta::new(
            __cgp::NestedShape::#shape,
            #meta_fn,
        ))));
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
    for field in &section.fields {
        if let Some(nested) = &field.nested {
            let ty = &nested.section;
            // Spanned at the field's type, so "not a section" points there.
            where_clause
                .predicates
                .push(syn::parse_quote_spanned!(ty.span()=> #ty: __cgp::ConfigSection));
        }
    }
    let fields = section.fields.iter().map(field_meta);
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
    quote! {
        #[automatically_derived]
        impl #impl_generics __cgp::ConfigSection for #ident #ty_generics #where_clause {
            const META: &'static __cgp::ConfigMeta = &#meta;
        }
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
