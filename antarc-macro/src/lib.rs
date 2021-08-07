#![feature(proc_macro_diagnostic)]

use proc_macro::Diagnostic;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data::Enum, DataEnum, DeriveInput, Field, Ident, Variant};

pub(crate) struct SingleFieldEnum {
    ident: Ident,
    variants: Vec<SingleFieldVariant>,
}

pub(crate) struct SingleFieldVariant {
    inner: Variant,
    field: Field,
}

pub(crate) fn get_variant(variant: &Variant) -> SingleFieldVariant {
    let field = variant.fields.iter().last().unwrap();

    let event_variant = SingleFieldVariant {
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
        let derive_input: DeriveInput = syn::parse2(input).unwrap();
        let data = derive_input.data;
        let ident = derive_input.ident;

        let single_field_enum = if let Enum(data_enum) = data {
            let DataEnum { variants, .. } = data_enum;

            let single_field_enum = SingleFieldEnum {
                ident,
                variants: variants
                    .iter()
                    .map(|variant| get_variant(variant))
                    .collect(),
            };

            single_field_enum
        } else {
            panic!("Only supports `Enum`s!");
        };

        let impl_variants = single_field_enum
            .variants
            .iter()
            .flat_map(|variant| {
                let variant_field_type = &variant.field.ty;
                let variant_field_name = &variant.field.ident;
                let variant_name = &variant.inner.ident;
                let enum_name = &single_field_enum.ident;

                let expand = quote! {
                    impl From<#variant_field_type> for #enum_name {
                        fn from(#variant_field_name: #variant_field_type) -> Self {
                            Self::#variant_name { #variant_field_name }
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
