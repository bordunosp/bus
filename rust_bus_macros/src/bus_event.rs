use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemStruct, parse_macro_input};

pub fn bus_event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemStruct);

    let required = ["Debug", "Clone", "Serialize", "Deserialize"];
    let mut missing = required.iter().collect::<std::collections::HashSet<_>>();

    for attr in &input.attrs {
        if attr.path().is_ident("derive") {
            let _ = attr.parse_nested_meta(|meta| {
                for &req in &required {
                    if meta.path.is_ident(req) {
                        missing.remove(&req);
                    }
                }
                Ok(())
            });
        }
    }

    if !missing.is_empty() {
        let mut to_add = Vec::new();
        if missing.contains(&"Debug") {
            to_add.push(quote!(::core::fmt::Debug));
        }
        if missing.contains(&"Clone") {
            to_add.push(quote!(::core::clone::Clone));
        }
        if missing.contains(&"Serialize") {
            to_add.push(quote!(::serde::Serialize));
        }
        if missing.contains(&"Deserialize") {
            to_add.push(quote!(::serde::Deserialize));
        }

        input
            .attrs
            .push(syn::parse_quote! { #[derive(#(#to_add),*)] });
    }

    input
        .attrs
        .push(syn::parse_quote! { #[serde(crate = "::serde")] });

    let struct_name = &input.ident;

    let expanded = quote! {
        #input

        impl #struct_name {
            pub const EVENT_IDENTITY: &'static str = concat!(
                module_path!(), "::",
                stringify!(#struct_name)
            );
        }

        impl rust_bus::contracts::bus_event::IBusEvent for #struct_name {
            const EVENT_IDENTITY: &'static str = Self::EVENT_IDENTITY;
        }
    };

    TokenStream::from(expanded)
}
