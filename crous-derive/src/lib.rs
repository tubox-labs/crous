//! # crous-derive
//!
//! Proc-macro crate providing `#[derive(Crous)]` and `#[derive(CrousSchema)]`
//! for automatic serialization with stable field IDs.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crous_derive::{Crous, CrousSchema};
//! use crous_core::Value;
//!
//! #[derive(Debug, PartialEq, Crous, CrousSchema)]
//! struct Person {
//!     #[crous(id = 1)] name: String,
//!     #[crous(id = 2)] age: u8,
//!     #[crous(id = 3)] tags: Vec<String>,
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Lit, parse_macro_input};

/// Derive the `Crous` trait for a struct, generating encode/decode
/// implementations with stable field IDs.
///
/// Each field must be annotated with `#[crous(id = N)]` where N is a
/// unique, stable integer identifier for schema evolution.
#[proc_macro_derive(Crous, attributes(crous))]
pub fn derive_crous(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Crous derive only supports structs with named fields"),
        },
        _ => panic!("Crous derive only supports structs"),
    };

    // Extract field names and their crous(id = N) attributes.
    let mut field_infos = Vec::new();
    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        let mut field_id: Option<u64> = None;

        for attr in &field.attrs {
            if attr.path().is_ident("crous") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("id") {
                        let value = meta.value()?;
                        let lit: Lit = value.parse()?;
                        if let Lit::Int(lit_int) = lit {
                            field_id = Some(lit_int.base10_parse().unwrap());
                        }
                    }
                    Ok(())
                })
                .ok();
            }
        }

        let id = field_id.unwrap_or_else(|| {
            // If no explicit id, use hash of field name as fallback.
            // This is not recommended but provides a default.
            let hash = xxhash_rust::xxh64::xxh64(field_name_str.as_bytes(), 0);
            hash & 0xFFFF // Use lower 16 bits.
        });

        field_infos.push((field_name.clone(), field_name_str, id));
    }

    // Generate schema fingerprint from type name + field IDs.
    let schema_str = format!(
        "{}:{}",
        name_str,
        field_infos
            .iter()
            .map(|(_, n, id)| format!("{n}={id}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let fingerprint = xxhash_rust::xxh64::xxh64(schema_str.as_bytes(), 0);

    // Generate to_crous_value: creates an Object with field name keys.
    let encode_fields: Vec<_> = field_infos
        .iter()
        .map(|(fname, fname_str, _id)| {
            quote! {
                (
                    #fname_str.to_string(),
                    crous_core::Crous::to_crous_value(&self.#fname)
                )
            }
        })
        .collect();

    // Generate from_crous_value: matches field names from Object entries.
    let decode_fields_init: Vec<_> = field_infos
        .iter()
        .map(|(fname, _fname_str, _id)| {
            quote! {
                let mut #fname = None;
            }
        })
        .collect();

    let decode_fields_match: Vec<_> = field_infos
        .iter()
        .map(|(fname, fname_str, _id)| {
            quote! {
                #fname_str => {
                    #fname = Some(crous_core::Crous::from_crous_value(v)?);
                }
            }
        })
        .collect();

    let decode_fields_unwrap: Vec<_> = field_infos
        .iter()
        .map(|(fname, fname_str, _id)| {
            quote! {
                #fname: #fname.ok_or_else(|| crous_core::CrousError::SchemaMismatch(
                    format!("missing field '{}' in {}", #fname_str, #name_str)
                ))?
            }
        })
        .collect();

    let expanded = quote! {
        impl crous_core::Crous for #name {
            fn to_crous_value(&self) -> crous_core::Value {
                crous_core::Value::Object(vec![
                    #(#encode_fields),*
                ])
            }

            fn from_crous_value(value: &crous_core::Value) -> crous_core::Result<Self> {
                match value {
                    crous_core::Value::Object(entries) => {
                        #(#decode_fields_init)*

                        for (k, v) in entries {
                            match k.as_str() {
                                #(#decode_fields_match)*
                                _ => {} // Skip unknown fields for forward compatibility.
                            }
                        }

                        Ok(Self {
                            #(#decode_fields_unwrap),*
                        })
                    }
                    _ => Err(crous_core::CrousError::SchemaMismatch(
                        format!("expected object for {}", #name_str)
                    )),
                }
            }

            fn schema_fingerprint() -> u64 {
                #fingerprint
            }

            fn type_name() -> &'static str {
                #name_str
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive `CrousSchema` — generates a `schema_info()` method that returns
/// metadata about the struct's field IDs and types.
///
/// This is a companion to `#[derive(Crous)]` and provides introspection.
#[proc_macro_derive(CrousSchema, attributes(crous))]
pub fn derive_crous_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let name_str = name.to_string();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("CrousSchema only supports structs with named fields"),
        },
        _ => panic!("CrousSchema only supports structs"),
    };

    let mut field_entries = Vec::new();
    for field in fields {
        let fname = field.ident.as_ref().unwrap().to_string();
        let _ftype = format!("{}", quote!(#(field.ty)));
        let mut fid: u64 = 0;

        for attr in &field.attrs {
            if attr.path().is_ident("crous") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("id") {
                        let value = meta.value()?;
                        let lit: Lit = value.parse()?;
                        if let Lit::Int(lit_int) = lit {
                            fid = lit_int.base10_parse().unwrap();
                        }
                    }
                    Ok(())
                })
                .ok();
            }
        }

        field_entries.push(quote! {
            (#fname, #fid)
        });
    }

    let expanded = quote! {
        impl #name {
            /// Returns schema metadata: pairs of (field_name, field_id).
            pub fn schema_info() -> &'static [(&'static str, u64)] {
                &[ #(#field_entries),* ]
            }

            /// Returns the type name.
            pub fn schema_type_name() -> &'static str {
                #name_str
            }
        }
    };

    TokenStream::from(expanded)
}
