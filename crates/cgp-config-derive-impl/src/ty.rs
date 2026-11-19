//! Type inspection: `Option<T>`, sequences, and maps, by their last path segment.

use syn::{GenericArgument, PathArguments, Type};

/// The generic arguments of `ty` when its last segment is one of `names`.
fn args_of<'a>(ty: &'a Type, names: &[&str]) -> Option<Vec<&'a Type>> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let last = path.path.segments.last()?;
    if !names.iter().any(|name| last.ident == name) {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    Some(
        args.args
            .iter()
            .filter_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .collect(),
    )
}

/// `T` of `Option<T>`.
pub(crate) fn option_inner(ty: &Type) -> Option<&Type> {
    match args_of(ty, &["Option"])?.as_slice() {
        [inner] => Some(inner),
        _ => None,
    }
}

/// `T` of `Vec<T>`, `VecDeque<T>`, `BTreeSet<T>`, `HashSet<T>`, `[T; N]`, `Box<[T]>`.
pub(crate) fn seq_inner(ty: &Type) -> Option<&Type> {
    if let Type::Array(array) = ty {
        return Some(&array.elem);
    }
    match args_of(
        ty,
        &[
            "Vec",
            "VecDeque",
            "LinkedList",
            "BTreeSet",
            "HashSet",
            "IndexSet",
        ],
    )?
    .as_slice()
    {
        [inner] => Some(inner),
        [inner, _hasher] => Some(inner),
        _ => None,
    }
}

/// `V` of `BTreeMap<K, V>`, `HashMap<K, V>`, `IndexMap<K, V>`.
pub(crate) fn map_value(ty: &Type) -> Option<&Type> {
    match args_of(ty, &["BTreeMap", "HashMap", "IndexMap"])?.as_slice() {
        [_key, value] | [_key, value, _] => Some(value),
        _ => None,
    }
}

/// The type as written, without the token-stream spacing (`Vec<String>`).
pub(crate) fn display(ty: &Type) -> String {
    let text = quote::quote!(#ty).to_string();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' ' {
            let prev = out.chars().last();
            let next = chars.peek().copied();
            let tight =
                |c: Option<char>| matches!(c, Some('<' | '>' | ':' | '&' | '(' | ')' | '[' | ']'));
            if tight(prev) || tight(next) || next == Some(',') {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::{display, map_value, option_inner, seq_inner};

    #[test]
    fn shapes() {
        let ty: syn::Type = parse_quote!(Option<Http>);
        assert!(option_inner(&ty).is_some());
        let ty: syn::Type = parse_quote!(std::vec::Vec<Server>);
        assert_eq!(display(seq_inner(&ty).unwrap()), "Server");
        let ty: syn::Type = parse_quote!([Server; 2]);
        assert!(seq_inner(&ty).is_some());
        let ty: syn::Type = parse_quote!(HashMap<String, Pool, S>);
        assert_eq!(display(map_value(&ty).unwrap()), "Pool");
        let ty: syn::Type = parse_quote!(BTreeMap<String, Vec<u8>>);
        assert_eq!(display(&ty), "BTreeMap<String, Vec<u8>>");
        let ty: syn::Type = parse_quote!(u16);
        assert!(
            option_inner(&ty).is_none() && seq_inner(&ty).is_none() && map_value(&ty).is_none()
        );
    }
}
