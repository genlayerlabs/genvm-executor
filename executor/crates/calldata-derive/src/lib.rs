mod attrs;
mod decode;
mod encode;
mod int_traits;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(Encode, attributes(calldata))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match encode::derive(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[proc_macro_derive(Decode, attributes(calldata))]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match decode::derive(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests {
    //! The derive entry points take `syn` types and return `syn::Result`, so the
    //! attribute rejection guards are testable directly -- no trybuild fixture and
    //! no pinned compiler output. Both `encode` and `decode` are asserted for
    //! every case, since the guards live on both paths.
    use super::{decode, encode};

    fn parse(src: &str) -> syn::DeriveInput {
        syn::parse_str(src).unwrap()
    }

    fn reject_msg(src: &str) -> (String, String) {
        let di = parse(src);
        let e = encode::derive(&di).err().expect("encode should reject");
        let d = decode::derive(&di).err().expect("decode should reject");
        (e.to_string(), d.to_string())
    }

    #[test]
    fn option_as_absence_ok_on_named_option_field() {
        let di = parse("struct S { #[calldata(option_as_absence)] a: Option<u32> }");
        assert!(encode::derive(&di).is_ok());
        assert!(decode::derive(&di).is_ok());
    }

    #[test]
    fn option_as_absence_rejected_on_non_option_field() {
        let (e, d) = reject_msg("struct S { #[calldata(option_as_absence)] a: u32 }");
        assert!(e.contains("Option<T>"), "{e}");
        assert!(d.contains("Option<T>"), "{d}");
    }

    #[test]
    fn option_as_absence_rejected_on_tuple_struct_field() {
        let (e, d) = reject_msg("struct S(#[calldata(option_as_absence)] Option<u32>);");
        assert!(e.contains("named struct fields"), "{e}");
        assert!(d.contains("named struct fields"), "{d}");
    }

    #[test]
    fn option_as_absence_rejected_on_enum_struct_variant_field() {
        let (e, d) = reject_msg("enum E { V { #[calldata(option_as_absence)] a: Option<u32> } }");
        assert!(e.contains("named struct fields"), "{e}");
        assert!(d.contains("named struct fields"), "{d}");
    }

    #[test]
    fn option_as_absence_rejected_on_enum_tuple_variant_field() {
        let (e, d) = reject_msg("enum E { V(#[calldata(option_as_absence)] Option<u32>) }");
        assert!(e.contains("named struct fields"), "{e}");
        assert!(d.contains("named struct fields"), "{d}");
    }

    #[test]
    fn option_as_absence_incompatible_with_default() {
        // Rejected in `FieldAttrs::from_ast`, so it surfaces on both paths.
        let (e, d) =
            reject_msg("struct S { #[calldata(option_as_absence, default = def)] a: Option<u32> }");
        assert!(e.contains("cannot be combined"), "{e}");
        assert!(d.contains("cannot be combined"), "{d}");
    }
}
