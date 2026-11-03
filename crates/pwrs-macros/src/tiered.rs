//! `#[tiered]`: one function body compiled once per instruction-set tier,
//! and a dispatcher that runs the copy for the tier `pwrs::cpu::level` or
//! `pwrs::cpu::arm::tier` returns, each computed once per process.

use proc_macro2::{TokenStream, TokenTree};
use quote::{format_ident, quote, ToTokens};
use syn::spanned::Spanned;
use syn::{FnArg, GenericParam, ItemFn, Pat, ReturnType, Type};

/// The x86-64 psABI levels above the baseline, widest first: the
/// `pwrs::cpu::Level` variant, and every target feature its body is
/// compiled with, its own and every lower level's.
pub const X86_64: [(&str, &str); 3] = [
    ("V4", "sse3,ssse3,sse4.1,sse4.2,popcnt,cmpxchg16b,avx,avx2,bmi1,bmi2,f16c,fma,lzcnt,movbe,xsave,avx512f,avx512bw,avx512cd,avx512dq,avx512vl"),
    ("V3", "sse3,ssse3,sse4.1,sse4.2,popcnt,cmpxchg16b,avx,avx2,bmi1,bmi2,f16c,fma,lzcnt,movbe,xsave"),
    ("V2", "sse3,ssse3,sse4.1,sse4.2,popcnt,cmpxchg16b"),
];

/// The AArch64 feature tiers above NEON, widest first: the
/// `pwrs::cpu::arm::Tier` variant, and every target feature its body is
/// compiled with.
pub const AARCH64: [(&str, &str); 4] = [("Sve2", "dotprod,i8mm,sve,sve2"), ("Sve", "dotprod,i8mm,sve"), ("I8mm", "dotprod,i8mm"), ("Dotprod", "dotprod")];

/// The two tables as an array expression of `(variant, features)` pairs,
/// for the test that checks them against `pwrs::cpu`'s own.
pub fn feature_lists() -> TokenStream {
    let x86 = X86_64.iter().map(|(v, f)| quote!((#v, #f)));
    let arm = AARCH64.iter().map(|(v, f)| quote!((#v, #f)));
    quote!(([#(#x86),*], [#(#arm),*]))
}

fn refuse(at: impl Spanned, text: &str) -> syn::Error {
    syn::Error::new(at.span(), text)
}

/// Whether `ty` contains an `impl Trait`: the keyword appears in a type
/// nowhere else.
fn has_impl_trait(ty: &Type) -> bool {
    fn scan(tokens: TokenStream) -> bool {
        tokens.into_iter().any(|t| match t {
            TokenTree::Ident(i) => i == "impl",
            TokenTree::Group(g) => scan(g.stream()),
            TokenTree::Punct(_) | TokenTree::Literal(_) => false,
        })
    }
    scan(ty.to_token_stream())
}

pub fn expand_tiered(attr: TokenStream, input: ItemFn) -> syn::Result<TokenStream> {
    if !attr.is_empty() {
        return Err(refuse(&attr, "#[tiered] takes no arguments"));
    }
    let sig = &input.sig;
    if let Some(a) = &sig.asyncness {
        return Err(refuse(a, "#[tiered] goes on a plain fn: an async fn's body runs as a future, outside any tier's target features"));
    }
    if let Some(c) = &sig.constness {
        return Err(refuse(c, "#[tiered] cannot go on a const fn, which no target feature may mark"));
    }
    if let Some(abi) = &sig.abi {
        return Err(refuse(abi, "#[tiered] goes on a Rust fn; give an extern fn a body that calls one"));
    }
    if let Some(v) = &sig.variadic {
        return Err(refuse(v, "#[tiered] cannot pass a variadic argument on to the tier bodies"));
    }
    if let ReturnType::Type(_, ty) = &sig.output
        && has_impl_trait(ty)
    {
        return Err(refuse(ty, "#[tiered] needs the return type named: each tier's body would return an impl Trait of its own"));
    }
    let mut args = Vec::new();
    let mut passed = Vec::new();
    for arg in &sig.inputs {
        let FnArg::Typed(typed) = arg else {
            return Err(refuse(arg, "#[tiered] goes on a free fn; a method can call one"));
        };
        if has_impl_trait(&typed.ty) {
            return Err(refuse(&typed.ty, "#[tiered] needs each argument's type named, so the dispatcher can pass it on; use a generic parameter in place of impl Trait"));
        }
        match &*typed.pat {
            Pat::Ident(name) if name.by_ref.is_none() && name.subpat.is_none() => {
                args.push(name.ident.clone());
                let mut plain = typed.clone();
                plain.pat = Box::new(Pat::Ident(syn::PatIdent { mutability: None, ..name.clone() }));
                passed.push(plain);
            }
            other => return Err(refuse(other, "#[tiered] needs each argument to be a name, as `values: &[f32]`, so the dispatcher can pass it on")),
        }
    }

    let generics = &sig.generics;
    let where_clause = &sig.generics.where_clause;
    let output = &sig.output;
    let unsafety = &sig.safety;
    let inputs = &sig.inputs;
    let block = &input.block;
    let named: Vec<TokenStream> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => Some(t.ident.to_token_stream()),
            GenericParam::Const(c) => Some(c.ident.to_token_stream()),
            GenericParam::Lifetime(_) => None,
        })
        .collect();
    let turbofish = if named.is_empty() { quote!() } else { quote!(::<#(#named),*>) };
    let call_body = if matches!(unsafety, syn::Safety::Unsafe(_)) { quote!(unsafe { __pwrs_body #turbofish (#(#args),*) }) } else { quote!(__pwrs_body #turbofish (#(#args),*)) };

    let ladder = |tiers: &[(&str, &str)], prefix: &str, path: TokenStream, answer: TokenStream| {
        let bodies = tiers.iter().map(|(variant, features)| {
            let name = format_ident!("__pwrs_{}_{}", prefix, variant.to_lowercase());
            quote! {
                #[target_feature(enable = #features)]
                unsafe fn #name #generics (#(#passed),*) #output #where_clause {
                    #call_body
                }
            }
        });
        let arms = tiers.iter().map(|(variant, _)| {
            let name = format_ident!("__pwrs_{}_{}", prefix, variant.to_lowercase());
            let variant = format_ident!("{}", variant);
            // The tier is reached only when the CPU and the operating
            // system offer every feature this body was compiled with.
            quote!(#path::#variant => unsafe { #name #turbofish (#(#args),*) },)
        });
        quote! {
            #(#bodies)*
            return match #answer {
                #(#arms)*
                _ => #call_body,
            };
        }
    };
    let x86 = ladder(&X86_64, "x86_64", quote!(::pwrs::cpu::Level), quote!(::pwrs::cpu::level()));
    let arm = ladder(&AARCH64, "aarch64", quote!(::pwrs::cpu::arm::Tier), quote!(::pwrs::cpu::arm::tier()));

    let attrs = &input.attrs;
    let vis = &input.vis;
    let ident = &sig.ident;
    Ok(quote! {
        #(#attrs)*
        #[allow(unreachable_code)]
        #vis #unsafety fn #ident #generics (#(#passed),*) #output #where_clause {
            #[inline(always)]
            #unsafety fn __pwrs_body #generics (#inputs) #output #where_clause #block

            #[cfg(target_arch = "x86_64")]
            {
                #x86
            }
            #[cfg(target_arch = "aarch64")]
            {
                #arm
            }
            #call_body
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{expand_tiered, AARCH64, X86_64};
    use crate::params::squeezed;

    fn expand(attr: &str, source: &str) -> syn::Result<String> {
        let attr: proc_macro2::TokenStream = attr.parse().expect("tokens");
        expand_tiered(attr, syn::parse_str(source).expect("a fn")).map(|tokens| squeezed(&tokens.to_string()))
    }

    /// Each tier's list contains every list below it, and the tiers are the
    /// psABI levels and the AArch64 feature tiers, widest first.
    #[test]
    fn each_tier_is_compiled_with_its_own_features_and_every_lower_tiers() {
        for tiers in [&X86_64[..], &AARCH64[..]] {
            for pair in tiers.windows(2) {
                let wider: Vec<&str> = pair[0].1.split(',').collect();
                assert!(pair[1].1.split(',').all(|f| wider.contains(&f)), "{} lacks a feature of {}", pair[0].0, pair[1].0);
            }
        }
        assert_eq!(X86_64.map(|t| t.0), ["V4", "V3", "V2"]);
        assert_eq!(AARCH64.map(|t| t.0), ["Sve2", "Sve", "I8mm", "Dotprod"]);
    }

    #[test]
    fn a_fn_gets_one_body_per_tier_and_a_dispatcher_over_them() {
        let source = "pub fn squares<T: Copy, const N: usize>(mut values: &[T], out: &mut [T; N]) -> usize { values.len() }";
        let tokens = match expand("", source) {
            Ok(tokens) => tokens,
            Err(e) => panic!("{source} was refused: {e}"),
        };
        for part in [
            "#[target_feature(enable=\"dotprod,i8mm,sve,sve2\")]unsafefn__pwrs_aarch64_sve2<T:Copy,constN:usize>(values:&[T],out:&mut[T;N])->usize",
            "::pwrs::cpu::Level::V4=>unsafe{__pwrs_x86_64_v4::<T,N>(values,out)},",
            "::pwrs::cpu::arm::Tier::Dotprod=>unsafe{__pwrs_aarch64_dotprod::<T,N>(values,out)},",
            "_=>__pwrs_body::<T,N>(values,out),",
            "#[inline(always)]fn__pwrs_body<T:Copy,constN:usize>(mutvalues:&[T],out:&mut[T;N])->usize{values.len()}",
            "pubfnsquares<T:Copy,constN:usize>(values:&[T],out:&mut[T;N])->usize",
        ] {
            assert!(tokens.contains(part), "{part}\n{tokens}");
        }
    }

    #[test]
    fn a_fn_the_dispatcher_cannot_pass_its_arguments_to_is_refused() {
        for (source, message) in [
            ("async fn f(x: u8) {}", "an async fn's body"),
            ("const fn f(x: u8) {}", "const fn"),
            ("extern \"C\" fn f(x: u8) {}", "give an extern fn a body"),
            ("fn f((a, b): (u8, u8)) {}", "each argument to be a name"),
            ("fn f(x: impl Copy) {}", "use a generic parameter in place of impl Trait"),
            ("fn f(x: u8) -> impl Copy { x }", "return type named"),
        ] {
            match expand("", source) {
                Ok(_) => panic!("{source} was accepted"),
                Err(e) => assert!(e.to_string().contains(message), "{source}: {e}"),
            }
        }
        match expand("v3", "fn f(x: u8) {}") {
            Ok(_) => panic!("an argument was accepted"),
            Err(e) => assert!(e.to_string().contains("takes no arguments"), "{e}"),
        }
    }
}
