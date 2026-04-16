//! Proc macros for the omnifs provider SDK.
//!
//! `#[provider]` processes an `impl TypeName { ... }` block, classifying
//! methods into lifecycle, resume, notify, route handlers, and helpers.
//! It generates WIT trait implementations, state management, dispatch
//! functions, and a route dispatch chain.
//!
//! `#[path("...")]` is a marker attribute consumed by `#[provider]`.
//! Using it outside a `#[provider]` impl block is a compile error.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    Attribute, FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr, Pat, ReturnType, Type,
    parse_macro_input,
};

// ---------------------------------------------------------------------------
// Template parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Segment {
    Literal(String),
    Capture(String),
    Rest(String),
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn parse_template(template: &str) -> Result<Vec<Segment>, String> {
    if template == "/" {
        return Ok(vec![]);
    }

    let raw = template.strip_prefix('/').unwrap_or(template);
    let parts: Vec<&str> = raw.split('/').collect();
    let mut segments = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        if part.starts_with("{*") && part.ends_with('}') {
            let name = &part[2..part.len() - 1];
            if name.is_empty() || !is_valid_ident(name) {
                return Err(format!("invalid rest capture name: `{name}`"));
            }
            if i != parts.len() - 1 {
                return Err(format!("rest capture {{*{name}}} must be the last segment"));
            }
            segments.push(Segment::Rest(name.to_string()));
        } else if part.starts_with('{') && part.ends_with('}') {
            let name = &part[1..part.len() - 1];
            if name.is_empty() || !is_valid_ident(name) {
                return Err(format!("invalid capture name: `{name}`"));
            }
            segments.push(Segment::Capture(name.to_string()));
        } else {
            segments.push(Segment::Literal((*part).to_string()));
        }
    }

    Ok(segments)
}

// ---------------------------------------------------------------------------
// Method classification
// ---------------------------------------------------------------------------

struct PathMethod {
    name: Ident,
    template: String,
    segments: Vec<Segment>,
    func: ImplItemFn,
}

struct ClassifiedMethods {
    init: Option<InitMethod>,
    capabilities: Option<ImplItemFn>,
    config_schema: Option<ImplItemFn>,
    resume: Option<ResumeMethod>,
    on_event: Option<ImplItemFn>,
    routes: Vec<PathMethod>,
    helpers: Vec<ImplItemFn>,
}

struct InitMethod {
    func: ImplItemFn,
    config_type: Type,
    state_type: Type,
}

struct ResumeMethod {
    func: ImplItemFn,
    continuation_type: Type,
}

fn extract_route_attr(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("route")
            && let Ok(lit) = attr.parse_args::<LitStr>()
        {
            return Some(lit.value());
        }
    }
    None
}

fn strip_route_attrs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| !attr.path().is_ident("route"));
}

/// Extract the parameter name from a function argument pattern.
fn param_name(arg: &FnArg) -> Option<String> {
    match arg {
        FnArg::Typed(pat_type) => {
            if let Pat::Ident(ident) = &*pat_type.pat {
                Some(ident.ident.to_string())
            } else {
                None
            }
        }
        FnArg::Receiver(_) => None,
    }
}

/// Extract the type from a function argument.
fn param_type(arg: &FnArg) -> Option<&Type> {
    match arg {
        FnArg::Typed(pat_type) => Some(&*pat_type.ty),
        FnArg::Receiver(_) => None,
    }
}

/// Check if a type is `&str` (reference to str).
fn is_str_ref(ty: &Type) -> bool {
    if let Type::Reference(r) = ty
        && let Type::Path(p) = &*r.elem
    {
        return p.path.is_ident("str");
    }
    false
}

/// Parse the return type of init: `(State, ProviderInfo)` -> extract State type.
fn extract_init_types(func: &ImplItemFn) -> Result<(Type, Type), syn::Error> {
    // First param is config type
    let config_type = func
        .sig
        .inputs
        .first()
        .and_then(param_type)
        .cloned()
        .ok_or_else(|| syn::Error::new(func.sig.span(), "init must have a config parameter"))?;

    // Return type is (State, ProviderInfo)
    let state_type = match &func.sig.output {
        ReturnType::Type(_, ty) => {
            if let Type::Tuple(tuple) = &**ty {
                if tuple.elems.len() == 2 {
                    tuple.elems[0].clone()
                } else {
                    return Err(syn::Error::new(
                        ty.span(),
                        "init must return (State, ProviderInfo)",
                    ));
                }
            } else {
                return Err(syn::Error::new(
                    ty.span(),
                    "init must return (State, ProviderInfo)",
                ));
            }
        }
        ReturnType::Default => {
            return Err(syn::Error::new(
                func.sig.span(),
                "init must return (State, ProviderInfo)",
            ));
        }
    };

    Ok((config_type, state_type))
}

/// Extract continuation type from resume's third parameter.
fn extract_continuation_type(func: &ImplItemFn) -> Result<Type, syn::Error> {
    // resume(id: u64, cont: C, outcome: EffectResult)
    // Third param (index 1 in inputs since no self) is the continuation type
    let args: Vec<_> = func.sig.inputs.iter().collect();
    if args.len() != 3 {
        return Err(syn::Error::new(
            func.sig.span(),
            "resume must have signature: fn resume(id: u64, cont: C, outcome: EffectResult) -> ProviderResponse",
        ));
    }
    param_type(args[1])
        .cloned()
        .ok_or_else(|| syn::Error::new(func.sig.span(), "cannot extract continuation type"))
}

fn classify_methods(items: Vec<ImplItem>) -> Result<ClassifiedMethods, syn::Error> {
    let mut init = None;
    let mut capabilities = None;
    let mut config_schema = None;
    let mut resume = None;
    let mut on_event = None;
    let mut routes = Vec::new();
    let mut helpers = Vec::new();

    for item in items {
        let ImplItem::Fn(mut func) = item else {
            continue;
        };

        let name = func.sig.ident.to_string();

        // Check for #[path] attribute
        if let Some(template) = extract_route_attr(&func.attrs) {
            strip_route_attrs(&mut func.attrs);
            let segments = parse_template(&template).map_err(|e| {
                syn::Error::new(func.sig.span(), format!("invalid path template: {e}"))
            })?;

            // Validate: parameter names must match capture names (after `op`)
            let capture_names: Vec<&str> = segments
                .iter()
                .filter_map(|s| match s {
                    Segment::Capture(n) | Segment::Rest(n) => Some(n.as_str()),
                    Segment::Literal(_) => None,
                })
                .collect();

            let param_names: Vec<String> = func
                .sig
                .inputs
                .iter()
                .skip(1) // skip `op`
                .filter_map(param_name)
                .collect();

            if capture_names.len() != param_names.len() {
                return Err(syn::Error::new(
                    func.sig.span(),
                    format!(
                        "path template has {} captures but method has {} parameters (after op)",
                        capture_names.len(),
                        param_names.len()
                    ),
                ));
            }

            for (cap, param) in capture_names.iter().zip(param_names.iter()) {
                if *cap != param {
                    return Err(syn::Error::new(
                        func.sig.span(),
                        format!("capture name `{cap}` does not match parameter name `{param}`"),
                    ));
                }
            }

            routes.push(PathMethod {
                name: func.sig.ident.clone(),
                template,
                segments,
                func,
            });
            continue;
        }

        match name.as_str() {
            "init" => {
                let (config_type, state_type) = extract_init_types(&func)?;
                init = Some(InitMethod {
                    func,
                    config_type,
                    state_type,
                });
            }
            "capabilities" => {
                capabilities = Some(func);
            }
            "get_config_schema" => {
                config_schema = Some(func);
            }
            "resume" => {
                let continuation_type = extract_continuation_type(&func)?;
                resume = Some(ResumeMethod {
                    func,
                    continuation_type,
                });
            }
            "on_event" => {
                on_event = Some(func);
            }
            _ => {
                helpers.push(func);
            }
        }
    }

    Ok(ClassifiedMethods {
        init,
        capabilities,
        config_schema,
        resume,
        on_event,
        routes,
        helpers,
    })
}

// ---------------------------------------------------------------------------
// Code generation: match wrappers
// ---------------------------------------------------------------------------

fn generate_match_wrapper(type_name: &Ident, route: &PathMethod) -> TokenStream2 {
    let wrapper_name = format_ident!("__match_{}", route.name);
    let method_name = &route.name;

    // Root path special case
    if route.segments.is_empty() {
        return quote! {
            fn #wrapper_name(op: omnifs_sdk::Op, path: &str) -> Option<omnifs_sdk::prelude::ProviderResponse> {
                if !path.is_empty() { return None; }
                #type_name::#method_name(op)
            }
        };
    }

    let has_rest = route.segments.iter().any(|s| matches!(s, Segment::Rest(_)));
    let fixed_count = route
        .segments
        .iter()
        .filter(|s| !matches!(s, Segment::Rest(_)))
        .count();

    // Length check
    let len_check = if has_rest {
        let min = fixed_count + 1; // at least one rest segment
        quote! { if segments.len() < #min { return None; } }
    } else {
        quote! { if segments.len() != #fixed_count { return None; } }
    };

    // Generate literal checks and capture bindings
    let mut literal_checks = Vec::new();
    let mut capture_bindings = Vec::new();
    let mut call_args = Vec::new();
    let mut seg_idx = 0usize;

    // Collect parameter types (skip `op`)
    let param_types: Vec<&Type> = route
        .func
        .sig
        .inputs
        .iter()
        .skip(1)
        .filter_map(param_type)
        .collect();

    let mut param_type_idx = 0;

    for segment in &route.segments {
        match segment {
            Segment::Literal(lit) => {
                literal_checks.push(quote! {
                    if segments[#seg_idx] != #lit { return None; }
                });
                seg_idx += 1;
            }
            Segment::Capture(name) => {
                let ident = format_ident!("{}", name);
                let ty = param_types[param_type_idx];

                if is_str_ref(ty) {
                    capture_bindings.push(quote! {
                        let #ident: &str = segments[#seg_idx];
                    });
                } else {
                    capture_bindings.push(quote! {
                        let #ident: #ty = segments[#seg_idx].parse().ok()?;
                    });
                }

                call_args.push(quote! { #ident });
                param_type_idx += 1;
                seg_idx += 1;
            }
            Segment::Rest(name) => {
                let ident = format_ident!("{}", name);
                let prefix_count = seg_idx;
                capture_bindings.push(quote! {
                    let rest_offset: usize = segments[..#prefix_count].iter().map(|s| s.len() + 1).sum();
                    let #ident: &str = &path[rest_offset..];
                });
                call_args.push(quote! { #ident });
                param_type_idx += 1;
                // rest is always last, no increment needed
            }
        }
    }

    quote! {
        fn #wrapper_name(op: omnifs_sdk::Op, path: &str) -> Option<omnifs_sdk::prelude::ProviderResponse> {
            let segments: Vec<&str> = path.split('/').collect();
            #len_check
            #(#literal_checks)*
            #(#capture_bindings)*
            #type_name::#method_name(op, #(#call_args),*)
        }
    }
}

// ---------------------------------------------------------------------------
// Code generation: dispatch chain
// ---------------------------------------------------------------------------

fn generate_dispatch_chain(routes: &[PathMethod]) -> TokenStream2 {
    let matchers: Vec<TokenStream2> = routes
        .iter()
        .map(|route| {
            let wrapper_name = format_ident!("__match_{}", route.name);
            quote! { .or_else(|| #wrapper_name(op, path)) }
        })
        .collect();

    quote! {
        fn __dispatch(op: omnifs_sdk::Op, path: &str) -> Option<omnifs_sdk::prelude::ProviderResponse> {
            None #(#matchers)*
        }
    }
}

// ---------------------------------------------------------------------------
// Code generation: WIT trait impls
// ---------------------------------------------------------------------------

fn generate_lifecycle_impl(
    type_name: &Ident,
    init: &InitMethod,
    _capabilities: &ImplItemFn,
    has_config_schema: bool,
    cont_type: &Type,
) -> TokenStream2 {
    let config_type = &init.config_type;
    let config_schema_body = if has_config_schema {
        quote! { #type_name::get_config_schema() }
    } else {
        quote! { omnifs_sdk::prelude::ConfigSchema { fields: vec![] } }
    };

    quote! {
        impl omnifs_sdk::exports::omnifs::provider::lifecycle::Guest for #type_name {
            fn initialize(config_bytes: Vec<u8>) -> omnifs_sdk::prelude::ProviderResponse {
                let config_str = match core::str::from_utf8(&config_bytes) {
                    Ok(s) => s,
                    Err(e) => return omnifs_sdk::prelude::ProviderResponse::Done(
                        omnifs_sdk::prelude::ActionResult::Err(format!("invalid UTF-8: {e}"))
                    ),
                };
                let config: #config_type = match omnifs_sdk::toml::from_str(config_str) {
                    Ok(c) => c,
                    Err(e) => return omnifs_sdk::prelude::ProviderResponse::Done(
                        omnifs_sdk::prelude::ActionResult::Err(format!("config error: {e}"))
                    ),
                };
                let (state, info) = #type_name::init(config);
                STATE.with(|s| {
                    *s.borrow_mut() = Some(omnifs_sdk::__internal::StateWrapper {
                        inner: state,
                        pending: omnifs_sdk::hashbrown::HashMap::new(),
                    });
                });
                omnifs_sdk::prelude::ProviderResponse::Done(
                    omnifs_sdk::prelude::ActionResult::ProviderInitialized(info)
                )
            }

            fn capabilities() -> omnifs_sdk::prelude::RequestedCapabilities {
                #type_name::capabilities()
            }

            fn shutdown() {
                STATE.with(|s| *s.borrow_mut() = None);
            }

            fn get_config_schema() -> omnifs_sdk::prelude::ConfigSchema {
                #config_schema_body
            }
        }

        impl omnifs_sdk::exports::omnifs::provider::resume::Guest for #type_name {
            fn resume(id: u64, outcome: omnifs_sdk::prelude::EffectResult) -> omnifs_sdk::prelude::ProviderResponse {
                let cont: #cont_type = match STATE.with(|s| {
                    let mut borrow = s.borrow_mut();
                    borrow.as_mut().and_then(|w| w.pending.remove(&id))
                }) {
                    Some(c) => c,
                    None => return omnifs_sdk::prelude::err("no pending continuation"),
                };
                #type_name::resume(id, cont, outcome)
            }

            fn cancel(id: u64) {
                STATE.with(|s| {
                    if let Some(w) = s.borrow_mut().as_mut() {
                        w.pending.remove(&id);
                    }
                });
            }
        }
    }
}

fn generate_browse_impl(type_name: &Ident) -> TokenStream2 {
    quote! {
        impl omnifs_sdk::exports::omnifs::provider::browse::Guest for #type_name {
            fn lookup_child(id: u64, parent_path: String, name: String) -> omnifs_sdk::prelude::ProviderResponse {
                let path = if parent_path.is_empty() { name } else { format!("{parent_path}/{name}") };
                __dispatch(omnifs_sdk::Op::Lookup(id), &path)
                    .unwrap_or_else(|| omnifs_sdk::prelude::ProviderResponse::Done(
                        omnifs_sdk::prelude::ActionResult::DirEntryOption(None)
                    ))
            }

            fn list_children(id: u64, path: String) -> omnifs_sdk::prelude::ProviderResponse {
                __dispatch(omnifs_sdk::Op::List(id), &path)
                    .unwrap_or_else(|| omnifs_sdk::prelude::err("not found"))
            }

            fn read_file(id: u64, path: String) -> omnifs_sdk::prelude::ProviderResponse {
                __dispatch(omnifs_sdk::Op::Read(id), &path)
                    .unwrap_or_else(|| omnifs_sdk::prelude::err("not found"))
            }

            fn open_file(_: u64, _: String) -> omnifs_sdk::prelude::ProviderResponse {
                omnifs_sdk::prelude::ProviderResponse::Done(
                    omnifs_sdk::prelude::ActionResult::FileOpened(1)
                )
            }

            fn read_chunk(_: u64, _: u64, _: u64, _: u32) -> omnifs_sdk::prelude::ProviderResponse {
                omnifs_sdk::prelude::ProviderResponse::Done(
                    omnifs_sdk::prelude::ActionResult::FileChunk(vec![])
                )
            }

            fn close_file(_: u64) {}
        }
    }
}

fn generate_notify_impl(type_name: &Ident, has_on_event: bool) -> TokenStream2 {
    let body = if has_on_event {
        quote! { #type_name::on_event(id, event) }
    } else {
        quote! {
            let _ = (id, event);
            omnifs_sdk::prelude::ProviderResponse::Done(omnifs_sdk::prelude::ActionResult::Ok)
        }
    };

    quote! {
        impl omnifs_sdk::exports::omnifs::provider::notify::Guest for #type_name {
            fn on_event(id: u64, event: omnifs_sdk::prelude::ProviderEvent) -> omnifs_sdk::prelude::ProviderResponse {
                #body
            }
        }
    }
}

fn generate_reconcile_impl(type_name: &Ident) -> TokenStream2 {
    quote! {
        impl omnifs_sdk::exports::omnifs::provider::reconcile::Guest for #type_name {
            fn plan_mutations(
                _id: u64,
                _changes: Vec<omnifs_sdk::prelude::FileChange>,
            ) -> omnifs_sdk::prelude::ProviderResponse {
                omnifs_sdk::prelude::err("reconcile not implemented")
            }

            fn execute(
                _id: u64,
                _mutation: omnifs_sdk::prelude::PlannedMutation,
            ) -> omnifs_sdk::prelude::ProviderResponse {
                omnifs_sdk::prelude::err("reconcile not implemented")
            }

            fn fetch_resource(
                _id: u64,
                _resource_path: String,
            ) -> omnifs_sdk::prelude::ProviderResponse {
                omnifs_sdk::prelude::err("reconcile not implemented")
            }

            fn list_scope(
                _id: u64,
                _scope: String,
            ) -> omnifs_sdk::prelude::ProviderResponse {
                omnifs_sdk::prelude::err("reconcile not implemented")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// State management generation
// ---------------------------------------------------------------------------

fn generate_state_management(state_type: &Type, cont_type: &Type) -> TokenStream2 {
    quote! {
        thread_local! {
            static STATE: core::cell::RefCell<Option<omnifs_sdk::__internal::StateWrapper<#state_type, #cont_type>>>
                = const { core::cell::RefCell::new(None) };
        }

        pub(crate) fn with_state<F, R>(f: F) -> Result<R, String>
        where
            F: FnOnce(&mut #state_type) -> R,
        {
            STATE.with(|s| {
                let mut borrow = s.borrow_mut();
                match borrow.as_mut() {
                    Some(wrapper) => Ok(f(&mut wrapper.inner)),
                    None => Err("provider not initialized".to_string()),
                }
            })
        }

        pub(crate) fn with_pending<F, R>(f: F) -> Result<R, String>
        where
            F: FnOnce(&mut omnifs_sdk::hashbrown::HashMap<u64, #cont_type>) -> R,
        {
            STATE.with(|s| {
                let mut borrow = s.borrow_mut();
                match borrow.as_mut() {
                    Some(wrapper) => Ok(f(&mut wrapper.pending)),
                    None => Err("provider not initialized".to_string()),
                }
            })
        }

        pub(crate) fn dispatch(id: u64, cont: #cont_type, effect: omnifs_sdk::prelude::SingleEffect) -> omnifs_sdk::prelude::ProviderResponse {
            match with_pending(|pending| pending.insert(id, cont)) {
                Ok(_) => omnifs_sdk::prelude::ProviderResponse::Effect(effect),
                Err(e) => omnifs_sdk::prelude::err(&e),
            }
        }

        pub(crate) fn dispatch_batch(id: u64, cont: #cont_type, effects: Vec<omnifs_sdk::prelude::SingleEffect>) -> omnifs_sdk::prelude::ProviderResponse {
            match with_pending(|pending| pending.insert(id, cont)) {
                Ok(_) => omnifs_sdk::prelude::ProviderResponse::Batch(effects),
                Err(e) => omnifs_sdk::prelude::err(&e),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main provider macro
// ---------------------------------------------------------------------------

/// Attribute macro for omnifs provider impl blocks.
///
/// Processes the impl block, classifying methods into lifecycle, resume,
/// notify, route handlers, and helpers. Generates WIT trait implementations,
/// state management, dispatch functions, and a route dispatch chain.
#[proc_macro_attribute]
pub fn provider(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    match provider_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn provider_impl(input: ItemImpl) -> Result<TokenStream2, syn::Error> {
    // Extract the type name
    let type_name = match &*input.self_ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.clone()),
        _ => None,
    }
    .ok_or_else(|| syn::Error::new(input.self_ty.span(), "expected a named type"))?;

    // Classify all methods
    let classified = classify_methods(input.items)?;

    // Validate required methods
    let init = classified
        .init
        .as_ref()
        .ok_or_else(|| syn::Error::new(type_name.span(), "missing required `init` method"))?;
    let caps = classified.capabilities.as_ref().ok_or_else(|| {
        syn::Error::new(type_name.span(), "missing required `capabilities` method")
    })?;
    let resume_method = classified
        .resume
        .as_ref()
        .ok_or_else(|| syn::Error::new(type_name.span(), "missing required `resume` method"))?;

    // Check for duplicate path templates
    let mut seen_templates = std::collections::HashSet::new();
    for route in &classified.routes {
        if !seen_templates.insert(&route.template) {
            return Err(syn::Error::new(
                route.func.sig.span(),
                format!("duplicate path template: {}", route.template),
            ));
        }
    }

    let state_type = &init.state_type;
    let cont_type = &resume_method.continuation_type;

    // Generate struct definition
    let struct_def = quote! { struct #type_name; };

    // Generate state management
    let state_mgmt = generate_state_management(state_type, cont_type);

    // Collect all original method definitions into an impl block
    let init_func = &init.func;
    let caps_func = caps;
    let config_schema_func = classified.config_schema.as_ref();
    let resume_func = &resume_method.func;
    let on_event_func = classified.on_event.as_ref();
    let helper_funcs = &classified.helpers;
    let route_funcs: Vec<&ImplItemFn> = classified.routes.iter().map(|r| &r.func).collect();

    let on_event_tokens: Vec<TokenStream2> = on_event_func.iter().map(|f| quote! { #f }).collect();
    let config_schema_tokens: Vec<TokenStream2> =
        config_schema_func.iter().map(|f| quote! { #f }).collect();

    let impl_block = quote! {
        impl #type_name {
            #init_func
            #caps_func
            #(#config_schema_tokens)*
            #resume_func
            #(#on_event_tokens)*
            #(#route_funcs)*
            #(#helper_funcs)*
        }
    };

    // Generate match wrappers
    let match_wrappers: Vec<TokenStream2> = classified
        .routes
        .iter()
        .map(|r| generate_match_wrapper(&type_name, r))
        .collect();

    // Generate dispatch chain
    let dispatch_chain = generate_dispatch_chain(&classified.routes);

    // Generate WIT trait impls
    let lifecycle_impl = generate_lifecycle_impl(
        &type_name,
        init,
        caps,
        classified.config_schema.is_some(),
        cont_type,
    );
    let browse_impl = generate_browse_impl(&type_name);
    let notify_impl = generate_notify_impl(&type_name, classified.on_event.is_some());
    let reconcile_impl = generate_reconcile_impl(&type_name);

    // Generate export macro call
    let export_call = quote! {
        omnifs_sdk::export!(#type_name with_types_in omnifs_sdk);
    };

    Ok(quote! {
        #struct_def
        #state_mgmt
        #impl_block
        #(#match_wrappers)*
        #dispatch_chain
        #lifecycle_impl
        #browse_impl
        #notify_impl
        #reconcile_impl
        #export_call
    })
}

/// Marker attribute for route handler methods inside `#[provider]` impl blocks.
///
/// Using this outside a `#[provider]` impl block is a compile error.
#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse the attribute to validate it's a string literal
    let _template = parse_macro_input!(attr as LitStr);
    let func = parse_macro_input!(item as ImplItemFn);

    // Emit a compile error: #[route] must be consumed by #[provider]
    let err = syn::Error::new(
        func.sig.span(),
        "#[route] can only be used inside an #[omnifs_sdk::provider] impl block",
    );
    err.to_compile_error().into()
}
