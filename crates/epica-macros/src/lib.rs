use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Expr, ExprLit, Fields, Lit, MetaNameValue,
    punctuated::Punctuated, Token,
};

/// Derive macro for Epica belief state types.
///
/// Annotate a struct with `#[derive(BeliefState)]` and mark individual fields
/// with `#[belief(...)]` to get automatic `BeliefQuad` conversion, typed
/// confidence accessors, a default `BehavioralContract`, and a `SchemaDescriptor`
/// for MCP Server Card generation.
///
/// # Attribute syntax
///
/// ```rust,ignore
/// #[derive(BeliefState, Debug)]
/// pub struct MyBeliefs {
///     #[belief(
///         confidence = 0.85,
///         source = "llm:inference",
///         decays_after = "10m",
///         prospect = true,
///         reflection_threshold = 0.15,
///         causal_parent = "user_intent",
///         governance = "human_approval",
///         audit = "full",
///         cache_affinity = "session",
///     )]
///     pub blockers: Vec<String>,
/// }
/// ```
///
/// | Attribute              | Type   | Default           | Effect |
/// |------------------------|--------|-------------------|--------|
/// | `confidence`           | f32    | 0.5               | Initial `fast_confidence` |
/// | `source`               | String | `"llm:inference"` | `BeliefValue` variant + `Provenance` |
/// | `decays_after`         | String | none              | Sets `node.ttl_ms` |
/// | `prospect`             | bool   | false             | Enables write-time indexing |
/// | `reflection_threshold` | f32    | 0.15              | Per-node System 2 τ |
/// | `causal_parent`        | String | none              | Adds `CausalEdge::InferredFrom` |
/// | `governance`           | String | none              | Governance policy hint |
/// | `audit`                | String | none              | Audit policy hint |
/// | `cache_affinity`       | String | none              | Cache hint (schema only) |
///
/// # Generated methods
///
/// - `to_belief_quad(&self) -> BeliefQuad`
/// - `from_belief_quad(quad: &BeliefQuad) -> Option<Self>`
/// - `field_confidence(quad, key) -> Option<(f32, Option<f32>)>` (generic)
/// - `{field}_confidence(quad) -> (f32, Option<f32>)` (one per belief field)
/// - `default_contract() -> BehavioralContract`
/// - `schema_descriptor() -> SchemaDescriptor`
#[proc_macro_derive(BeliefState, attributes(belief))]
pub fn derive_belief_state(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    match impl_belief_state(ast) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

// ── Core derive implementation ────────────────────────────────────────────────

fn impl_belief_state(ast: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &ast.ident;
    let name_str = name.to_string();
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();

    let named_fields = match &ast.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => return Err(syn::Error::new(
                Span::call_site(), "BeliefState requires named fields")),
        },
        _ => return Err(syn::Error::new(
            Span::call_site(), "BeliefState can only be derived on structs")),
    };

    // Parse belief fields.
    let mut belief_fields: Vec<BeliefFieldMeta> = Vec::new();
    let mut all_field_idents: Vec<syn::Ident> = Vec::new();

    for field in named_fields {
        let ident = field.ident.as_ref().unwrap().clone();
        all_field_idents.push(ident.clone());
        if let Some(attr) = field.attrs.iter().find(|a| a.path().is_ident("belief")) {
            belief_fields.push(parse_belief_attr(ident, attr)?);
        }
    }

    // ── to_belief_quad(): insert all nodes, then add causal edges ──────────────
    let insert_stmts: Vec<TokenStream2> = belief_fields.iter().map(gen_insert_stmt).collect();

    let causal_edge_stmts: Vec<TokenStream2> = belief_fields.iter()
        .filter_map(|f| {
            let parent_str = f.causal_parent.as_ref()?;
            let child_id  = format_ident!("__id_{}", f.ident);
            let parent_id = format_ident!("__id_{}", parent_str);
            Some(quote! {
                __quad.add_causal_edge(
                    #parent_id,
                    #child_id,
                    epica_core::CausalEdge::InferredFrom { premises: vec![#parent_id] },
                );
            })
        })
        .collect();

    // ── from_belief_quad(): extract each belief field ─────────────────────────
    let from_quad_stmts: Vec<TokenStream2> = belief_fields.iter().map(|f| {
        let key_str = f.ident.to_string();
        let ident   = &f.ident;
        quote! {
            let #ident = __quad.iter()
                .find(|(_, n)| n.key == #key_str)
                .and_then(|(_, n)| match &n.value {
                    epica_core::BeliefValue::Inferred(v)
                    | epica_core::BeliefValue::Deterministic(v) => {
                        serde_json::from_value(v.clone()).ok()
                    }
                    epica_core::BeliefValue::Asserted(s) => {
                        serde_json::from_value(
                            serde_json::Value::String(s.clone())
                        ).ok()
                    }
                    epica_core::BeliefValue::Reference(_) => None,
                });
        }
    }).collect();

    let belief_ident_set: std::collections::HashSet<String> =
        belief_fields.iter().map(|f| f.ident.to_string()).collect();

    let all_fields_init: Vec<TokenStream2> = all_field_idents.iter().map(|fname| {
        if belief_ident_set.contains(&fname.to_string()) {
            quote! { #fname: #fname.unwrap_or_default() }
        } else {
            quote! { #fname: ::std::default::Default::default() }
        }
    }).collect();

    // ── Per-field typed confidence accessors ──────────────────────────────────
    let typed_accessors: Vec<TokenStream2> = belief_fields.iter().map(|f| {
        let key_str     = f.ident.to_string();
        let method_name = format_ident!("{}_confidence", f.ident);
        quote! {
            /// Returns `(fast_confidence, slow_confidence)` for this belief field.
            pub fn #method_name(quad: &epica_core::BeliefQuad) -> (f32, ::std::option::Option<f32>) {
                quad.iter()
                    .find(|(_, n)| n.key == #key_str)
                    .map(|(_, n)| (n.fast_confidence, n.slow_confidence))
                    .unwrap_or((0.0, None))
            }
        }
    }).collect();

    // ── Generic field_confidence() ────────────────────────────────────────────
    let field_conf_arms: Vec<TokenStream2> = belief_fields.iter().map(|f| {
        let key_str = f.ident.to_string();
        quote! {
            #key_str => {
                __quad.iter()
                    .find(|(_, n)| n.key == #key_str)
                    .map(|(_, n)| (n.fast_confidence, n.slow_confidence))
            }
        }
    }).collect();

    // ── default_contract() ────────────────────────────────────────────────────
    let contract_preconditions: Vec<TokenStream2> = belief_fields.iter().map(|f| {
        let key_str = f.ident.to_string();
        quote! {
            __contract.preconditions.push(::std::boxed::Box::new(
                epica_contracts::BeliefExists { key: #key_str.to_string() }
            ));
        }
    }).collect();

    let contract_invariants: Vec<TokenStream2> = belief_fields.iter().map(|f| {
        let key_str   = f.ident.to_string();
        let threshold = f.confidence * 0.5_f32;
        let vclass    = if f.governance.as_deref() == Some("human_approval") {
            quote! { epica_contracts::ViolationClass::Hard }
        } else {
            quote! { epica_contracts::ViolationClass::Soft }
        };
        quote! {
            __contract.invariants.push(epica_contracts::SessionInvariant {
                predicate: ::std::boxed::Box::new(
                    epica_contracts::MinConfidence {
                        key: #key_str.to_string(),
                        threshold: #threshold,
                    }
                ),
                class: #vclass,
                max_intervention_gap_steps: 3,
            });
        }
    }).collect();

    let governance_stmts: Vec<TokenStream2> = belief_fields.iter()
        .filter(|f| f.governance.as_deref() == Some("human_approval"))
        .map(|f| {
            let key_str = f.ident.to_string();
            quote! {
                __contract.governance.require_human_approval.push(#key_str.to_string());
            }
        })
        .collect();

    // ── schema_descriptor() ───────────────────────────────────────────────────
    let schema_entries: Vec<TokenStream2> = belief_fields.iter().map(|f| {
        let key_str    = f.ident.to_string();
        let source     = &f.source;
        let confidence = f.confidence;
        let ttl_ms_tok = match f.ttl_ms {
            Some(ms) => quote! { ::std::option::Option::Some(#ms as u64) },
            None     => quote! { ::std::option::Option::None },
        };
        let prospect   = f.prospect;
        let rt         = f.reflection_threshold;
        let causal_p   = opt_str_tok(&f.causal_parent);
        let governance = opt_str_tok(&f.governance);
        let audit      = opt_str_tok(&f.audit);
        let cache_aff  = opt_str_tok(&f.cache_affinity);
        quote! {
            epica_core::BeliefFieldDescriptor {
                key:                  #key_str.to_string(),
                source:               #source.to_string(),
                initial_confidence:   #confidence,
                ttl_ms:               #ttl_ms_tok,
                prospect:             #prospect,
                reflection_threshold: #rt,
                causal_parent:        #causal_p,
                governance:           #governance,
                audit:                #audit,
                cache_affinity:       #cache_aff,
            }
        }
    }).collect();

    // ── Assemble the impl block ───────────────────────────────────────────────
    Ok(quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// Convert belief-annotated fields into a [`epica_core::BeliefQuad`].
            ///
            /// Non-belief fields are ignored.  Causal edges specified via
            /// `causal_parent` are added after all nodes have been inserted.
            pub fn to_belief_quad(&self) -> epica_core::BeliefQuad {
                let mut __quad = epica_core::BeliefQuad::new();
                #(#insert_stmts)*
                #(#causal_edge_stmts)*
                __quad
            }

            /// Reconstruct this struct from belief values in `quad`.
            ///
            /// Non-belief fields are initialised with `Default::default()`.
            pub fn from_belief_quad(
                __quad: &epica_core::BeliefQuad,
            ) -> ::std::option::Option<Self> {
                #(#from_quad_stmts)*
                ::std::option::Option::Some(Self { #(#all_fields_init,)* })
            }

            /// Returns `(fast_confidence, slow_confidence)` for the given belief key.
            ///
            /// Returns `None` for keys not annotated with `#[belief(...)]`.
            pub fn field_confidence(
                __quad: &epica_core::BeliefQuad,
                key: &str,
            ) -> ::std::option::Option<(f32, ::std::option::Option<f32>)> {
                match key {
                    #(#field_conf_arms,)*
                    _ => None,
                }
            }

            #(#typed_accessors)*

            /// Build a default [`epica_contracts::BehavioralContract`] for this struct.
            ///
            /// Customise the returned contract before attaching it to a
            /// `BeliefRuntime` via `with_contract()`.
            pub fn default_contract() -> epica_contracts::BehavioralContract {
                let mut __contract = epica_contracts::BehavioralContract {
                    domain:        #name_str.to_string(),
                    preconditions: ::std::vec::Vec::new(),
                    invariants:    ::std::vec::Vec::new(),
                    governance:    epica_contracts::GovernancePolicies::default(),
                    recovery:      epica_contracts::RecoveryPolicy::RollbackToLastCheckpoint,
                    p:     0.99,
                    delta: 1,
                    k:     100,
                    alpha: 0.05,
                    gamma: 0.5,
                };
                #(#contract_preconditions)*
                #(#contract_invariants)*
                #(#governance_stmts)*
                __contract
            }

            /// Generate a [`epica_core::SchemaDescriptor`] for MCP Server Card discovery.
            pub fn schema_descriptor() -> epica_core::SchemaDescriptor {
                epica_core::SchemaDescriptor {
                    name:    #name_str.to_string(),
                    beliefs: ::std::vec![#(#schema_entries,)*],
                }
            }
        }
    })
}

// ── Insert statement generation ───────────────────────────────────────────────

fn gen_insert_stmt(f: &BeliefFieldMeta) -> TokenStream2 {
    let ident      = &f.ident;
    let key_str    = ident.to_string();
    let id_ident   = format_ident!("__id_{}", ident);
    let confidence = f.confidence;
    let tau        = f.reflection_threshold;
    let prospect   = f.prospect;
    let source     = f.source.as_str();

    let provenance = gen_provenance(source);

    let belief_value = if source == "user" {
        quote! {
            epica_core::BeliefValue::Asserted(
                serde_json::to_string(&self.#ident).unwrap_or_default()
            )
        }
    } else if source == "deterministic" || source.starts_with("env:") {
        quote! { epica_core::BeliefValue::Deterministic(__json) }
    } else {
        quote! { epica_core::BeliefValue::Inferred(__json) }
    };

    let ttl_stmt = match f.ttl_ms {
        Some(ms) => quote! { __node.ttl_ms = ::std::option::Option::Some(#ms as u64); },
        None     => quote! {},
    };

    quote! {
        let #id_ident = {
            let __json = serde_json::to_value(&self.#ident)
                .unwrap_or(serde_json::Value::Null);
            let mut __node = epica_core::BeliefNode::new(
                #key_str,
                #belief_value,
                #provenance,
                #confidence,
            );
            __node.reflection_threshold = #tau;
            __node.prospect = #prospect;
            #ttl_stmt
            __quad.insert(__node)
        };
    }
}

fn gen_provenance(source: &str) -> TokenStream2 {
    if source == "user" {
        quote! { epica_core::Provenance::UserStatement { turn: 0 } }
    } else if source == "deterministic" || source.starts_with("env:") {
        quote! {
            epica_core::Provenance::ToolResult {
                tool:    #source.to_string(),
                call_id: uuid::Uuid::new_v4(),
            }
        }
    } else {
        quote! {
            epica_core::Provenance::LlmInference {
                model:       #source.to_string(),
                call_id:     uuid::Uuid::new_v4(),
                prompt_hash: 0,
            }
        }
    }
}

// ── Attribute parsing ─────────────────────────────────────────────────────────

const DEFAULT_TAU: f32 = 0.15;

struct BeliefFieldMeta {
    ident:                syn::Ident,
    confidence:           f32,
    source:               String,
    ttl_ms:               Option<u64>,
    prospect:             bool,
    reflection_threshold: f32,
    causal_parent:        Option<String>,
    governance:           Option<String>,
    audit:                Option<String>,
    cache_affinity:       Option<String>,
}

fn parse_belief_attr(ident: syn::Ident, attr: &syn::Attribute) -> syn::Result<BeliefFieldMeta> {
    let mut confidence:           f32            = 0.5;
    let mut source:               String         = "llm:inference".into();
    let mut ttl_ms:               Option<u64>    = None;
    let mut prospect:             bool           = false;
    let mut reflection_threshold: f32            = DEFAULT_TAU;
    let mut causal_parent:        Option<String> = None;
    let mut governance:           Option<String> = None;
    let mut audit:                Option<String> = None;
    let mut cache_affinity:       Option<String> = None;

    let pairs = attr.parse_args_with(
        Punctuated::<MetaNameValue, Token![,]>::parse_terminated,
    )?;

    for pair in pairs {
        let key = pair.path
            .get_ident()
            .ok_or_else(|| syn::Error::new_spanned(&pair.path, "expected identifier"))?
            .to_string();

        match key.as_str() {
            "confidence" => {
                confidence = lit_f32(&pair.value, "confidence")?;
            }
            "source" => {
                source = lit_str(&pair.value, "source")?;
            }
            "decays_after" => {
                let s = lit_str(&pair.value, "decays_after")?;
                ttl_ms = Some(parse_duration_ms(&s).ok_or_else(|| {
                    syn::Error::new_spanned(
                        &pair.value,
                        r#"invalid duration; expected e.g. "30s", "10m", "1h", "2d""#,
                    )
                })?);
            }
            "prospect" => {
                prospect = lit_bool(&pair.value, "prospect")?;
            }
            "reflection_threshold" => {
                reflection_threshold = lit_f32(&pair.value, "reflection_threshold")?;
            }
            "causal_parent" => {
                causal_parent = Some(lit_str(&pair.value, "causal_parent")?);
            }
            "governance" => {
                governance = Some(lit_str(&pair.value, "governance")?);
            }
            "audit" => {
                audit = Some(lit_str(&pair.value, "audit")?);
            }
            "cache_affinity" => {
                cache_affinity = Some(lit_str(&pair.value, "cache_affinity")?);
            }
            other => {
                return Err(syn::Error::new_spanned(
                    &pair.path,
                    format!("unknown belief attribute `{other}`"),
                ));
            }
        }
    }

    Ok(BeliefFieldMeta {
        ident,
        confidence,
        source,
        ttl_ms,
        prospect,
        reflection_threshold,
        causal_parent,
        governance,
        audit,
        cache_affinity,
    })
}

// ── Literal helpers ───────────────────────────────────────────────────────────

fn lit_f32(expr: &Expr, attr: &str) -> syn::Result<f32> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Float(f), .. }) => {
            f.base10_parse::<f32>().map_err(|_| {
                syn::Error::new_spanned(expr, format!("`{attr}` must be a valid f32"))
            })
        }
        Expr::Lit(ExprLit { lit: Lit::Int(i), .. }) => {
            i.base10_parse::<f32>().map_err(|_| {
                syn::Error::new_spanned(expr, format!("`{attr}` must be a valid number"))
            })
        }
        _ => Err(syn::Error::new_spanned(
            expr,
            format!("`{attr}` must be a float literal (e.g. `0.85`)"),
        )),
    }
}

fn lit_str(expr: &Expr, attr: &str) -> syn::Result<String> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Ok(s.value()),
        _ => Err(syn::Error::new_spanned(
            expr,
            format!("`{attr}` must be a string literal"),
        )),
    }
}

fn lit_bool(expr: &Expr, attr: &str) -> syn::Result<bool> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Bool(b), .. }) => Ok(b.value),
        _ => Err(syn::Error::new_spanned(
            expr,
            format!("`{attr}` must be a bool literal (`true` or `false`)"),
        )),
    }
}

// ── Duration parser ───────────────────────────────────────────────────────────

fn parse_duration_ms(s: &str) -> Option<u64> {
    if s.len() < 2 {
        return None;
    }
    let (digits, suffix) = s.split_at(s.len() - 1);
    let n: u64 = digits.parse().ok()?;
    match suffix {
        "s" => Some(n.saturating_mul(1_000)),
        "m" => Some(n.saturating_mul(60_000)),
        "h" => Some(n.saturating_mul(3_600_000)),
        "d" => Some(n.saturating_mul(86_400_000)),
        _   => None,
    }
}

// ── Token helpers ─────────────────────────────────────────────────────────────

fn opt_str_tok(opt: &Option<String>) -> TokenStream2 {
    match opt {
        Some(s) => quote! { ::std::option::Option::Some(#s.to_string()) },
        None    => quote! { ::std::option::Option::None },
    }
}
