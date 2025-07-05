use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use syn::{ItemImpl, Meta, PathArguments, parse_macro_input};

fn hash_type_name(ty: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    ty.hash(&mut hasher);
    hasher.finish()
}

fn parse_mode(meta: Meta) -> String {
    match meta {
        Meta::Path(path) => {
            if path.is_ident("factory") {
                "factory".to_string()
            } else {
                panic!("Expected #[BusEventHandler] or #[BusEventHandler(factory)]");
            }
        }
        _ => panic!("Expected #[BusEventHandler] or #[BusEventHandler(factory)]"),
    }
}

pub fn bus_event_handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mode = if attr.is_empty() {
        "default".to_string()
    } else {
        let meta = parse_macro_input!(attr as Meta);
        parse_mode(meta)
    };

    let input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

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
        panic!("Expected generic arguments in IEventHandler");
    };

    let mut args = generic_args.args.iter();
    let event_ty = args.next().expect("Missing TEvent").clone();
    let error_ty = args.next().expect("Missing TError").clone();

    let type_name = quote!(#self_ty).to_string();
    let hash = hash_type_name(&type_name);
    let fn_name = format_ident!(
        "__register_event_{}_{}",
        type_name
            .replace("::", "_")
            .replace('<', "_")
            .replace('>', "_"),
        hash
    );

    let factory_expr = match mode.as_str() {
        "default" => quote! {
            || ::std::boxed::Box::pin(async { Ok(::core::default::Default::default()) })
        },
        "factory" => quote! {
            || ::std::boxed::Box::pin(<#self_ty as ::bus::core::factory::EventProvidesFactory<#self_ty, #event_ty, #error_ty>>::factory())
        },
        _ => panic!("Unknown mode, use one of: default, factory for #[BusEventHandler]"),
    };

    let expanded = quote! {
        #input

        #[doc(hidden)]
        #[::ctor::ctor]
        fn #fn_name() {
            let _ = ::bus::core::registry::event_handler::<#self_ty, #event_ty, #error_ty, _, _>(#factory_expr);
        }
    };

    TokenStream::from(expanded)
}
