use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use syn::{ItemImpl, parse_macro_input};

fn hash_type_name(ty: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    ty.hash(&mut hasher);
    hasher.finish()
}

pub fn bus_event_pipeline(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

    let type_name = quote!(#self_ty).to_string();
    let hash = hash_type_name(&type_name);

    let fn_name = format_ident!(
        "__register_event_pipeline_{}_{}",
        type_name
            .replace("::", "_")
            .replace('<', "_")
            .replace('>', "_"),
        hash
    );

    let expanded = quote! {
        #input

        #[doc(hidden)]
        #[::ctor::ctor]
        fn #fn_name() {
            ::bus::core::registry::event_pipeline(|| ::std::sync::Arc::new(#self_ty::default()));
        }
    };

    TokenStream::from(expanded)
}
