use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use syn::{ItemImpl, PathArguments, parse_macro_input};

fn hash_type_name(ty: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    ty.hash(&mut hasher);
    hasher.finish()
}

pub fn bus_event_database_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

    // Витягуємо типи з impl IEventHandlerDatabase<TEvent, TError>
    let trait_path = input
        .trait_
        .as_ref()
        .expect("Expected trait impl")
        .1
        .segments
        .last()
        .expect("Expected trait path")
        .arguments
        .clone();

    let PathArguments::AngleBracketed(generic_args) = trait_path else {
        panic!("Expected generic arguments in IEventHandlerDatabase");
    };

    let mut args = generic_args.args.iter();
    let event_ty = args.next().expect("Missing TEvent").clone();
    let error_ty = args.next().expect("Missing TError").clone();

    let type_name = quote!(#self_ty).to_string();
    let hash = hash_type_name(&type_name);
    let fn_name = format_ident!(
        "__register_event_db_{}_{}",
        type_name
            .replace("::", "_")
            .replace('<', "_")
            .replace('>', "_"),
        hash
    );
    let factory_fn_name = format_ident!(
        "__factory_event_db_{}_{}",
        type_name
            .replace("::", "_")
            .replace('<', "_")
            .replace('>', "_"),
        hash
    );

    let factory_fn = quote! {
        #[doc(hidden)]
        fn #factory_fn_name() -> impl ::core::future::Future<Output = Result<#self_ty, #error_ty>> {
            <#self_ty as ::bus::core::factory::EventDatabaseProvidesFactory<#self_ty, #event_ty, #error_ty>>::factory()
        }
    };

    let expanded = quote! {
        #input

        #factory_fn

        #[doc(hidden)]
        #[::ctor::ctor]
        fn #fn_name() {
            let _ = ::bus::core::registry::event_database_handler::<#self_ty, #event_ty, #error_ty, _, _>(#factory_fn_name);
        }
    };

    TokenStream::from(expanded)
}
