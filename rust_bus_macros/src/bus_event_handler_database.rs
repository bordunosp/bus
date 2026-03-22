use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemImpl, PathArguments, parse_macro_input};

pub(crate) fn bus_event_handler_database_sea(item: TokenStream) -> TokenStream {
    _bus_event_handler_database(item, true, false, false)
}

pub(crate) fn bus_event_handler_database_sqlx_pg(item: TokenStream) -> TokenStream {
    _bus_event_handler_database(item, false, true, false)
}

pub(crate) fn bus_event_handler_database_sqlx_mysql(item: TokenStream) -> TokenStream {
    _bus_event_handler_database(item, false, false, true)
}

fn emit_err<T: quote::ToTokens>(tokens: T, msg: &str) -> TokenStream {
    syn::Error::new_spanned(tokens, msg)
        .to_compile_error()
        .into()
}

fn _bus_event_handler_database(
    item: TokenStream,
    is_sea: bool,
    is_sqlx_pg: bool,
    is_sqlx_mysql: bool,
) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

    let trait_info = match input.trait_.as_ref() {
        Some(t) => t,
        None => {
            return emit_err(
                self_ty,
                "Issue: This attribute must be applied to an 'impl IEventHandlerDatabase' block.",
            );
        }
    };

    let trait_segment = match trait_info.1.segments.last() {
        Some(seg) => seg,
        None => {
            return emit_err(
                &trait_info.1,
                "Issue: Trait path is empty.\n\
                 How to fix: Provide a valid trait implementation.",
            );
        }
    };

    let mut types = Vec::new();
    if let PathArguments::AngleBracketed(args) = &trait_segment.arguments {
        for arg in &args.args {
            if let syn::GenericArgument::Type(t) = arg {
                types.push(t);
            }
        }
    }

    if types.len() != 1 {
        return emit_err(
            trait_segment,
            "Issue: Expected exactly one generic argument for IEventHandlerDatabase (the Event type).\n\
             Expected: IEventHandlerDatabase<MyEvent>\n\
             How to fix: Check your trait implementation signature.",
        );
    }
    let event_ty = types[0];

    let db_ty = if is_sea {
        quote! { sea_orm::DatabaseConnection }
    } else if is_sqlx_pg {
        quote! { sqlx::PgPool }
    } else if is_sqlx_mysql {
        quote! { sqlx::MySqlPool }
    } else {
        return emit_err(
            self_ty,
            "Issue: Database type not specified. \nUse one of: sea-orm-postgres or sea-orm-mysql or sqlx-postgres or sqlx-mysql.",
        );
    };

    let expanded = quote! {
        #input

        impl #self_ty {
            pub const HANDLER_IDENTITY: &'static str = concat!(module_path!(), "::", stringify!(#self_ty));
        }

        rust_bus::inventory::submit! {
            rust_bus::dispatch::registration::EventDatabaseHandlerRegistration {
                handler_identity: <#self_ty>::HANDLER_IDENTITY,
                event_identity: <#event_ty as rust_bus::contracts::bus_event::IBusEvent>::EVENT_IDENTITY,
                queue: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::QUEUE,
                priority: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::PRIORITY,
                max_attempts: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::MAX_ATTEMPTS,
                execution_timeout: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::EXECUTION_TIMEOUT,
                tags: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::TAGS,
                unique: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::UNIQUE,
                schedule_in: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::SCHEDULE_IN,
                next_attempt_at: <#self_ty as rust_bus::contracts::event_handler_database::IEventHandlerDatabase<#event_ty>>::next_attempt_at,
                execute: |db_ptr, payload, meta_ptr| {
                    let db_addr = db_ptr as usize;
                    let meta_addr = meta_ptr as usize;
                    let payload_owned = payload.clone();

                    Box::pin(async move {
                        let handler = <#self_ty as Default>::default();
                        let db = unsafe { &*(db_addr as *const #db_ty) };

                        let event: #event_ty = serde_json::from_value(payload_owned)
                            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

                        let meta = unsafe { &*(meta_addr as *const rust_bus::contracts::meta::BusMetadata) };

                        handler.handle(db, &event, meta).await
                    })
                }
            }
        }
    };

    TokenStream::from(expanded)
}
