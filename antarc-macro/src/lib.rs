#![feature(proc_macro_diagnostic)]

use std::iter::FromIterator;

use proc_macro::Diagnostic;
use proc_macro2::{Delimiter, Group, Span, TokenStream, TokenTree};
use quote::quote;
use syn::{Data::Enum, DataEnum, DeriveInput, Field, Path, TypePath, Variant};

pub(crate) struct EventEnum {
    variants: Vec<EventVariant>,
}

pub(crate) struct EventVariant {
    inner: Variant,
    field: Field,
}

pub(crate) fn get_event_variant(variant: &Variant) -> EventVariant {
    let field = variant.fields.iter().last().unwrap();

    let event_variant = EventVariant {
        inner: variant.clone(),
        field: field.clone(),
    };

    event_variant
}

#[proc_macro_derive(FromScheduled)]
pub fn derive_from_scheduled(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = proc_macro2::TokenStream::from(input);
    let diag = Diagnostic::new(proc_macro::Level::Error, format!("FOOOOO \n\n FOOOO"));
    diag.emit();
    let output: proc_macro2::TokenStream = {
        let DeriveInput {
            attrs, ident, data, ..
        } = syn::parse2(input).unwrap();

        let event_enum = if let Enum(data_enum) = data {
            let DataEnum { variants, .. } = data_enum;

            let event = EventEnum {
                variants: variants
                    .iter()
                    .map(|variant| get_event_variant(variant))
                    .collect(),
            };

            event
        } else {
            panic!("Only supports `Enum`s!");
        };

        let impl_variants = event_enum
            .variants
            .iter()
            .flat_map(|variant| {
                let variant_field_type = &variant.field.ty;
                let variant_field_name = &variant.field.ident;
                let variant_name = &variant.inner.ident;

                let expand = quote! {
                    impl From<#variant_field_type> for ScheduleEvent {
                        fn from(#variant_field_name: #variant_field_type) -> Self {
                            Self::#variant_name { scheduled }
                        }
                    }
                };

                // panic!("{}", expand);

                expand
            })
            .collect::<TokenStream>();

        impl_variants.into()
    };

    proc_macro::TokenStream::from(output)
}
