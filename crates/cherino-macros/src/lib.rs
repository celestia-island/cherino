//! Proc macros for the cherino container toolkit.
//!
//! - [`Getters`] — derive macro that generates `fn field_name(&self) -> &T`
//!   accessor methods on structs, with `#[getter(skip)]` and
//!   `#[getter(rename = "…")]`.
#![allow(clippy::type_complexity)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Field, Lit, Type, parse_macro_input};

#[proc_macro_derive(Getters, attributes(getter))]
pub fn derive_getters(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            syn::Fields::Named(named) => &named.named,
            _ => return quote! { compile_error!("Getters only supports structs with named fields"); }.into(),
        },
        _ => return quote! { compile_error!("Getters only supports structs"); }.into(),
    };

    let getters: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .filter(|f| !has_getter_skip(&f.attrs))
        .map(generate_getter)
        .collect();

    let expanded = quote! {
        impl #name {
            #(#getters)*
        }
    };

    expanded.into()
}

fn has_getter_skip(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("getter") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                found = true;
            }
            Ok(())
        });
        found
    })
}

fn get_getter_rename(attrs: &[Attribute]) -> Option<proc_macro2::Ident> {
    for attr in attrs {
        if !attr.path().is_ident("getter") {
            continue;
        }
        let mut result: Option<proc_macro2::Ident> = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value: Lit = meta.value()?.parse()?;
                if let Lit::Str(s) = value {
                    result = Some(syn::Ident::new(&s.value(), s.span()));
                }
            }
            Ok(())
        });
        if result.is_some() {
            return result;
        }
    }
    None
}

fn generate_getter(field: &Field) -> proc_macro2::TokenStream {
    let field_ident = match field.ident.as_ref() {
        Some(ident) => ident,
        // generate_getter is only called on named fields; skip unnamed ones.
        None => return quote! {},
    };
    let method_name = get_getter_rename(&field.attrs).unwrap_or_else(|| field_ident.clone());

    let ty = &field.ty;

    let (return_expr, return_ty) = if is_string_type(ty) {
        (quote! { self.#field_ident.as_str() }, quote! { &str })
    } else if is_vec_type(ty) {
        let inner = extract_vec_inner(ty);
        (
            quote! { self.#field_ident.as_slice() },
            quote! { &[#inner] },
        )
    } else if is_option_string(ty) {
        (
            quote! { self.#field_ident.as_deref() },
            quote! { Option<&str> },
        )
    } else if is_option_type(ty) {
        let inner = extract_option_inner(ty);
        (
            quote! { self.#field_ident.as_ref() },
            quote! { Option<&#inner> },
        )
    } else if is_copy_type(ty) || is_ref_type(ty) {
        (quote! { self.#field_ident }, quote! { #ty })
    } else {
        (quote! { &self.#field_ident }, quote! { &#ty })
    };

    quote! {
        pub fn #method_name(&self) -> #return_ty {
            #return_expr
        }
    }
}

fn type_matches(ty: &Type, segment: &str) -> bool {
    if let Type::Path(type_path) = ty {
        let last = type_path.path.segments.last();
        last.map(|s| s.ident == segment).unwrap_or(false)
    } else {
        false
    }
}

fn is_string_type(ty: &Type) -> bool {
    type_matches(ty, "String")
}

fn is_vec_type(ty: &Type) -> bool {
    type_matches(ty, "Vec")
}

fn is_option_type(ty: &Type) -> bool {
    type_matches(ty, "Option")
}

fn is_option_string(ty: &Type) -> bool {
    if !is_option_type(ty) {
        return false;
    }
    let inner = extract_option_inner(ty);
    is_string_type(&inner)
}

fn is_copy_type(ty: &Type) -> bool {
    let copy_types = [
        "bool", "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "f32", "f64",
    ];
    if let Type::Path(type_path) = ty {
        let last = type_path.path.segments.last();
        last.map(|s| copy_types.contains(&s.ident.to_string().as_str()))
            .unwrap_or(false)
    } else {
        false
    }
}

fn is_ref_type(ty: &Type) -> bool {
    matches!(ty, Type::Reference(_))
}

fn extract_generic_inner(ty: &Type, outer: &str) -> Type {
    if let Type::Path(type_path) = ty
        && let Some(seg) = type_path.path.segments.last()
        && seg.ident == outer
        && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return inner.clone();
    }
    syn::parse_quote! { _ }
}

fn extract_vec_inner(ty: &Type) -> Type {
    extract_generic_inner(ty, "Vec")
}

fn extract_option_inner(ty: &Type) -> Type {
    extract_generic_inner(ty, "Option")
}
