use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, GenericArgument, ItemFn, PatType, PathArguments, Type, parse_macro_input};

pub(crate) fn bus_event_handler(item: TokenStream) -> TokenStream {
    _bus_event_handler(item, false, false, false, false)
}
pub(crate) fn bus_event_handler_context(item: TokenStream) -> TokenStream {
    _bus_event_handler(item, true, false, false, false)
}

pub(crate) fn bus_event_handler_sea(item: TokenStream) -> TokenStream {
    _bus_event_handler(item, false, true, false, false)
}

pub(crate) fn bus_event_handler_sqlx_pg(item: TokenStream) -> TokenStream {
    _bus_event_handler(item, false, false, true, false)
}
pub(crate) fn bus_event_handler_sqlx_mysql(item: TokenStream) -> TokenStream {
    _bus_event_handler(item, false, false, false, true)
}

fn emit_err<T: quote::ToTokens>(tokens: T, msg: &str) -> TokenStream {
    syn::Error::new_spanned(tokens, msg)
        .to_compile_error()
        .into()
}

fn _bus_event_handler(
    item: TokenStream,
    is_context: bool,
    is_sea: bool,
    is_sqlx_pg: bool,
    is_sqlx_mysql: bool,
) -> TokenStream {
    let input_fn_stream = proc_macro2::TokenStream::from(item.clone());
    let input_fn = parse_macro_input!(item as ItemFn);

    let fn_name = &input_fn.sig.ident;
    let inputs = &input_fn.sig.inputs;
    let args: Vec<_> = inputs.iter().collect();

    if input_fn.sig.asyncness.is_none() {
        return emit_err(
            input_fn.sig.fn_token,
            "Issue: Handler must be an 'async' function.\nHow to fix: Add 'async' keyword before 'fn'.",
        );
    }

    let res = (|| -> Result<(proc_macro2::TokenStream, proc_macro2::TokenStream), TokenStream> {
        let (ctx_call_ty, event_base_ty) = if is_context {
            if args.len() != 2 {
                return Err(emit_err(
                    inputs,
                    "Issue: Context Handler requires exactly 2 arguments.\nExpected: (ctx: &YourContext, event: &YourEvent)",
                ));
            }

            let c_ty = match args[0] {
                FnArg::Typed(PatType { ty, .. }) => ty,
                _ => {
                    return Err(emit_err(
                        args[0],
                        "First argument must be a typed Context reference.",
                    ));
                }
            };

            let e_ty = match args[1] {
                FnArg::Typed(PatType { ty, .. }) => ty,
                _ => {
                    return Err(emit_err(
                        args[1],
                        "Second argument must be a typed Event reference.",
                    ));
                }
            };

            let c_clean = clean_type(c_ty);
            let mut e_base = e_ty.clone();
            if let Type::Reference(r) = &*e_base {
                *e_base = (*r.elem).clone();
            }
            let e_clean = clean_type(&e_base);

            (c_clean, e_clean)
        } else {
            let e_ty = if is_sea || is_sqlx_pg || is_sqlx_mysql {
                if args.len() != 3 {
                    return Err(emit_err(
                        inputs,
                        "Issue: DB Handler requires exactly 3 arguments.\nExpected: (txn: &Tx, event: &Event, meta: &BusMetadata)",
                    ));
                }
                match args[1] {
                    FnArg::Typed(PatType { ty, .. }) => ty,
                    _ => {
                        return Err(emit_err(
                            args[1],
                            "Second argument must be a typed Event reference.",
                        ));
                    }
                }
            } else {
                if args.len() != 2 {
                    return Err(emit_err(
                        inputs,
                        "Issue: Standard Handler requires exactly 2 arguments.\nExpected: (event: &Event, meta: &BusMetadata)",
                    ));
                }
                match args[0] {
                    FnArg::Typed(PatType { ty, .. }) => ty,
                    _ => {
                        return Err(emit_err(
                            args[0],
                            "First argument must be a typed Event reference.",
                        ));
                    }
                }
            };

            let mut e_base = e_ty.clone();
            if let Type::Reference(r) = &*e_base {
                *e_base = (*r.elem).clone();
            }

            (quote! { () }, clean_type(&e_base))
        };

        Ok((ctx_call_ty, event_base_ty))
    })();

    let (ctx_call_ty, event_base_ty) = match res {
        Ok(v) => v,
        Err(e) => return e,
    };

    let handler_identity = quote! { concat!(module_path!(), "::", stringify!(#fn_name)) };
    let context_identity = quote! { concat!(module_path!(), "::", stringify!(#ctx_call_ty)) };

    let execute_logic = if is_context {
        quote! {
            context_identity: #context_identity,
            execute: |raw_ctx, event_ptr| {
                let raw_ptr_addr = raw_ctx.ptr as usize;
                let raw_is_mut = raw_ctx.is_mutable;
                let event_addr = event_ptr;

                Box::pin(async move {
                    let inner_raw_ctx = rust_bus::contracts::ctx::RawContext {
                        ptr: raw_ptr_addr as *mut (),
                        is_mutable: raw_is_mut,
                    };
                    let inner_event_ptr = event_addr as *const #event_base_ty;

                    let ctx = unsafe {
                        <#ctx_call_ty as rust_bus::contracts::ctx::FromContext>::from_raw(inner_raw_ctx)?
                    };
                    let event = unsafe { &*inner_event_ptr };

                    match #fn_name(ctx, event).await {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e.into()),
                    }
                })
            },
        }
    } else {
        let txn_unpack = if is_sea {
            quote! { let txn = unsafe { &*(txn_addr as *const sea_orm::DatabaseTransaction ) }; }
        } else if is_sqlx_pg {
            quote! { let txn = unsafe { &mut *(txn_addr as *mut sqlx::Transaction<'_, sqlx::Postgres>) }; }
        } else if is_sqlx_mysql {
            quote! { let txn = unsafe { &mut *(txn_addr as *mut sqlx::Transaction<'_, sqlx::MySql>) }; }
        } else {
            quote! {}
        };

        if is_sea || is_sqlx_pg || is_sqlx_mysql {
            quote! {
                execute: |txn_ptr, event_ptr, meta_ptr| {
                    let txn_addr = txn_ptr;
                    let event_addr = event_ptr;
                    let meta_addr = meta_ptr;

                    Box::pin(async move {
                        let inner_event_ptr = event_addr as *const #event_base_ty;
                        let inner_meta_ptr = meta_addr as *const rust_bus::contracts::meta::BusMetadata;

                        let event = unsafe { &*inner_event_ptr };
                        let metadata = unsafe { &*inner_meta_ptr };

                        #txn_unpack

                        match #fn_name(txn, event, metadata).await {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.into()),
                        }
                    })
                },
            }
        } else {
            quote! {
                execute: |event_ptr, meta_ptr| {
                    let event_addr = event_ptr;
                    let meta_addr = meta_ptr;

                    Box::pin(async move {
                        let inner_event_ptr = event_addr as *const #event_base_ty;
                        let inner_meta_ptr = meta_addr as *const rust_bus::contracts::meta::BusMetadata;

                        let event = unsafe { &*inner_event_ptr };
                        let metadata = unsafe { &*inner_meta_ptr };

                        match #fn_name(event, metadata).await {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.into()),
                        }
                    })
                },
            }
        }
    };

    TokenStream::from(quote! {
        #input_fn_stream

        rust_bus::inventory::submit! {
            rust_bus::dispatch::registration::EventHandlerRegistration {
                handler_identity: #handler_identity,
                event_identity: <#event_base_ty as rust_bus::contracts::bus_event::IBusEvent>::EVENT_IDENTITY,
                #execute_logic
            }
        }
    })
}

fn clean_type(ty: &Type) -> proc_macro2::TokenStream {
    match ty {
        Type::Reference(r) => {
            let inner = clean_type(&r.elem);
            let mutability = &r.mutability;
            quote! { & #mutability #inner }
        }
        Type::Path(p) => {
            let mut new_path = p.clone();
            for segment in new_path.path.segments.iter_mut() {
                if let PathArguments::AngleBracketed(ref mut args) = segment.arguments {
                    for arg in args.args.iter_mut() {
                        match arg {
                            GenericArgument::Lifetime(lt) => {
                                lt.ident = syn::Ident::new("_", lt.ident.span());
                            }
                            GenericArgument::Type(inner_ty) => {
                                let cleaned = clean_type(inner_ty);
                                *inner_ty = syn::parse2(cleaned).unwrap();
                            }
                            _ => {}
                        }
                    }
                }
            }
            quote! { #new_path }
        }
        _ => quote! { #ty },
    }
}
