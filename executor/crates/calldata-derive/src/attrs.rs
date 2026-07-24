use syn::Lit;

/// Container-level attributes (`#[calldata(...)]` on struct/enum).
pub struct ContainerAttrs {
    /// For enums: internally tagged with this field name.
    /// `#[calldata(tag = "type")]`
    pub tag: Option<String>,
    /// For enums: untagged representation.
    /// `#[calldata(untagged)]`
    pub untagged: bool,
}

/// Field/variant-level attributes (`#[calldata(...)]` on fields or enum variants).
pub struct FieldAttrs {
    /// `#[calldata(rename = "wire_name")]`
    pub rename: Option<String>,
    /// `#[calldata(serialize_with = path::to::fn)]`
    pub serialize_with: Option<syn::Path>,
    /// `#[calldata(deserialize_with = path::to::fn)]`
    pub deserialize_with: Option<syn::Path>,
    /// `#[calldata(default = path::to::fn)]`
    pub default: Option<syn::Path>,
    /// `#[calldata(option_as_absence)]`: an `Option<T>` field whose `None` is a
    /// *missing map key* rather than a present `null`. Decode never yields
    /// `FieldMissing`; encode omits the key (and it is left out of the map
    /// length). The field must be spelled `Option<T>`; the wire value is the
    /// inner `T`.
    pub option_as_absence: bool,
}

/// Extract `T` from a field type spelled `Option<T>`. Returns `None` for any
/// other shape (used to reject `option_as_absence` on non-`Option` fields).
pub fn option_inner_ty(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(tp) = ty else {
        return None;
    };
    if tp.qself.is_some() {
        return None;
    }
    let seg = tp.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

impl ContainerAttrs {
    pub fn from_ast(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut tag = None;
        let mut untagged = false;

        for attr in attrs {
            if !attr.path().is_ident("calldata") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("tag") {
                    let value = meta.value()?;
                    let lit: Lit = value.parse()?;
                    if let Lit::Str(s) = lit {
                        tag = Some(s.value());
                    } else {
                        return Err(meta.error("expected string literal for `tag`"));
                    }
                    return Ok(());
                }
                if meta.path.is_ident("untagged") {
                    untagged = true;
                    return Ok(());
                }
                Err(meta.error("unknown calldata container attribute"))
            })?;
        }

        if tag.is_some() && untagged {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`tag` and `untagged` are mutually exclusive",
            ));
        }

        Ok(ContainerAttrs { tag, untagged })
    }
}

impl FieldAttrs {
    pub fn from_ast(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut rename = None;
        let mut serialize_with = None;
        let mut deserialize_with = None;
        let mut default = None;
        let mut option_as_absence = false;

        for attr in attrs {
            if !attr.path().is_ident("calldata") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    let value = meta.value()?;
                    let lit: Lit = value.parse()?;
                    if let Lit::Str(s) = lit {
                        rename = Some(s.value());
                    } else {
                        return Err(meta.error("expected string literal for `rename`"));
                    }
                    return Ok(());
                }
                if meta.path.is_ident("serialize_with") {
                    let value = meta.value()?;
                    serialize_with = Some(value.parse::<syn::Path>()?);
                    return Ok(());
                }
                if meta.path.is_ident("deserialize_with") {
                    let value = meta.value()?;
                    deserialize_with = Some(value.parse::<syn::Path>()?);
                    return Ok(());
                }
                if meta.path.is_ident("default") {
                    let value = meta.value()?;
                    default = Some(value.parse::<syn::Path>()?);
                    return Ok(());
                }
                if meta.path.is_ident("option_as_absence") {
                    option_as_absence = true;
                    return Ok(());
                }
                Err(meta.error("unknown calldata field attribute"))
            })?;
        }

        if option_as_absence
            && (default.is_some() || serialize_with.is_some() || deserialize_with.is_some())
        {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`option_as_absence` cannot be combined with `default`, `serialize_with`, or `deserialize_with`",
            ));
        }

        Ok(FieldAttrs {
            rename,
            serialize_with,
            deserialize_with,
            default,
            option_as_absence,
        })
    }

    /// `option_as_absence` is only implemented for *named* struct fields; every
    /// other field position (tuple structs, all enum-variant shapes) ignores it
    /// silently, which would apply present-`null` semantics with a mismatched map
    /// length. Reject it there instead.
    pub fn reject_option_as_absence_here(&self, span: impl quote::ToTokens) -> syn::Result<()> {
        if self.option_as_absence {
            return Err(syn::Error::new_spanned(
                span,
                "`option_as_absence` is only supported on named struct fields",
            ));
        }
        Ok(())
    }

    /// The wire name for a struct field.
    pub fn wire_name(&self, ident: &syn::Ident) -> String {
        self.rename.clone().unwrap_or_else(|| ident.to_string())
    }

    /// The wire name for an enum variant.
    pub fn variant_wire_name(&self, ident: &syn::Ident) -> String {
        self.rename.clone().unwrap_or_else(|| ident.to_string())
    }
}
