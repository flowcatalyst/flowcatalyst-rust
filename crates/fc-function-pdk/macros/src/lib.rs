//! The `#[handler]` attribute of `fc-function-pdk`. Use it through the PDK
//! (`#[fc_function_pdk::handler]`), not this crate directly: the code it
//! writes names `::fc_function_pdk`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, spanned::Spanned, FnArg, ItemFn};

/// Makes a function the component's `wasi:http/incoming-handler` export.
///
/// The function takes the [`Request`] and, optionally, the [`Context`], may be
/// `async` or not, and returns anything that implements `HandlerOutput`:
/// a `Response`, or a `Result<Response, E>` whose `E: Display` (the PDK's
/// `Error`, `anyhow::Error`, any `std::error::Error`). An `Err` answers
/// Java's `fail`: `500` with `{"error":"<the error's message>"}`.
///
/// ```ignore
/// #[fc_function_pdk::handler]
/// async fn handle(req: Request, ctx: Context) -> Result<Response, Error> { … }
/// ```
///
/// One per component: a component has exactly one incoming handler.
///
/// [`Request`]: https://docs.rs/fc-function-pdk/latest/fc_function_pdk/struct.Request.html
/// [`Context`]: https://docs.rs/fc-function-pdk/latest/fc_function_pdk/struct.Context.html
#[proc_macro_attribute]
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let attr = proc_macro2::TokenStream::from(attr);
        return syn::Error::new(attr.span(), "#[handler] takes no arguments")
            .to_compile_error()
            .into();
    }
    let function = parse_macro_input!(item as ItemFn);
    match expand(&function) {
        Ok(tokens) => tokens.into(),
        Err(error) => {
            let error = error.to_compile_error();
            quote!(#function #error).into()
        }
    }
}

fn expand(function: &ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let sig = &function.sig;
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            sig.generics.span(),
            "a #[handler] function cannot be generic",
        ));
    }
    if let Some(receiver) = sig.receiver() {
        return Err(syn::Error::new(
            receiver.span(),
            "a #[handler] is a free function, not a method",
        ));
    }
    if sig.variadic.is_some() || sig.unsafety.is_some() || sig.abi.is_some() {
        return Err(syn::Error::new(
            sig.span(),
            "a #[handler] is a plain (optionally async) Rust function",
        ));
    }
    let arguments = sig.inputs.iter().filter(|a| matches!(a, FnArg::Typed(_)));
    let (params, call) = match arguments.count() {
        1 => (quote!(|request, _|), quote!((request))),
        2 => (quote!(|request, context|), quote!((request, context))),
        _ => {
            return Err(syn::Error::new(
                sig.inputs.span(),
                "a #[handler] takes (Request) or (Request, Context)",
            ))
        }
    };
    let name = &sig.ident;
    let future = if sig.asyncness.is_some() {
        quote!(#name #call)
    } else {
        quote!(::core::future::ready(#name #call))
    };
    let export = format_ident!("__FcFunctionPdkHandler_{}", name);
    let keep = format_ident!(
        "__FC_FUNCTION_PDK_HANDLER_{}",
        name.to_string().to_uppercase()
    );
    Ok(quote! {
        #function

        #[doc(hidden)]
        #[allow(non_camel_case_types, dead_code)]
        struct #export;

        impl ::fc_function_pdk::__private::IncomingHandler for #export {
            fn handle(
                request: ::fc_function_pdk::__private::IncomingRequest,
                out: ::fc_function_pdk::__private::ResponseOutparam,
            ) {
                ::fc_function_pdk::__private::serve(request, out, #params #future);
            }
        }

        // Only a component exports the handler: on the host target (a
        // function's own unit tests, `cargo build` without `--target`) the
        // export's symbol names do not link into a native library. There a
        // `#[used]` reference keeps the handler (and all it calls) live, so
        // a native build does not call the function's code dead.
        #[cfg(target_arch = "wasm32")]
        ::fc_function_pdk::__private::wasip2::http::proxy::export!(#export);
        #[cfg(not(target_arch = "wasm32"))]
        #[used]
        #[doc(hidden)]
        static #keep: fn(
            ::fc_function_pdk::__private::IncomingRequest,
            ::fc_function_pdk::__private::ResponseOutparam,
        ) = <#export as ::fc_function_pdk::__private::IncomingHandler>::handle;
    })
}
