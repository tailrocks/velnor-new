//! Conservative cfg evaluation for a Cargo `test` compilation profile.

use std::error::Error;

use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, Token};

use super::Possibility;

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) fn attrs_possible_in_test(attributes: &[Attribute]) -> Outcome<bool> {
    Ok(attrs_possibility_in_test(attributes)? != Possibility::Never)
}

pub(super) fn attrs_possibility_in_test(attributes: &[Attribute]) -> Outcome<Possibility> {
    attributes
        .iter()
        .try_fold(Possibility::Always, |state, attribute| {
            Ok::<_, Box<dyn Error>>(combine(state, meta_cfg_state(&attribute.meta)?))
        })
}

pub(super) fn meta_marks_test(meta: &Meta) -> Outcome<bool> {
    Ok(meta_test_state(meta)? != Possibility::Never)
}

fn meta_test_state(meta: &Meta) -> Outcome<Possibility> {
    match meta {
        Meta::Path(path)
            if path.segments.last().is_some_and(|segment| {
                matches!(
                    segment.ident.to_string().as_str(),
                    "test" | "rstest" | "test_case"
                )
            }) =>
        {
            Ok(Possibility::Always)
        }
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            let nested = nested_meta(&list.tokens)?;
            let (condition, attributes) =
                nested.split_first().ok_or("cfg_attr has no condition")?;
            let applies = test_condition_state(condition)?;
            let possible = metas_cfg_state(attributes)?;
            if possible == Possibility::Never {
                return Ok(Possibility::Never);
            }
            let marks_test = metas_test_state(attributes)?;
            Ok(combine(applies, combine(possible, marks_test)))
        }
        _ => Ok(Possibility::Never),
    }
}

fn metas_test_state(attributes: &[Meta]) -> Outcome<Possibility> {
    attributes
        .iter()
        .try_fold(Possibility::Never, |state, attribute| {
            Ok(or(state, meta_test_state(attribute)?))
        })
}

fn meta_cfg_state(meta: &Meta) -> Outcome<Possibility> {
    match meta {
        Meta::List(list) if list.path.is_ident("cfg") => {
            let nested = nested_meta(&list.tokens)?;
            let condition = nested.first().ok_or("cfg has no predicate")?;
            test_condition_state(condition)
        }
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            let nested = nested_meta(&list.tokens)?;
            let (condition, attributes) =
                nested.split_first().ok_or("cfg_attr has no condition")?;
            let condition = test_condition_state(condition)?;
            let attributes = metas_cfg_state(attributes)?;
            Ok(match condition {
                Possibility::Never => Possibility::Always,
                Possibility::Always => attributes,
                Possibility::Sometimes if attributes == Possibility::Always => Possibility::Always,
                Possibility::Sometimes => Possibility::Sometimes,
            })
        }
        _ => Ok(Possibility::Always),
    }
}

fn metas_cfg_state(attributes: &[Meta]) -> Outcome<Possibility> {
    attributes
        .iter()
        .try_fold(Possibility::Always, |state, attribute| {
            Ok::<_, Box<dyn Error>>(combine(state, meta_cfg_state(attribute)?))
        })
}

pub(super) fn test_condition_state(meta: &Meta) -> Outcome<Possibility> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Ok(Possibility::Always),
        Meta::List(list) if list.path.is_ident("all") => {
            let mut result = Possibility::Always;
            for nested in nested_meta(&list.tokens)? {
                result = combine(result, test_condition_state(&nested)?);
            }
            Ok(result)
        }
        Meta::List(list) if list.path.is_ident("any") => {
            let mut result = Possibility::Never;
            for nested in nested_meta(&list.tokens)? {
                result = or(result, test_condition_state(&nested)?);
            }
            Ok(result)
        }
        Meta::List(list) if list.path.is_ident("not") => {
            let nested = nested_meta(&list.tokens)?;
            let condition = nested.first().ok_or("cfg not has no predicate")?;
            Ok(negate(test_condition_state(condition)?))
        }
        Meta::NameValue(value) if value.path.is_ident("test") => Ok(Possibility::Never),
        _ => Ok(Possibility::Sometimes),
    }
}

fn nested_meta(tokens: &proc_macro2::TokenStream) -> Outcome<Vec<Meta>> {
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    Ok(parser.parse2(tokens.clone())?.into_iter().collect())
}

fn combine(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Never, _) | (_, Possibility::Never) => Possibility::Never,
        (Possibility::Always, Possibility::Always) => Possibility::Always,
        _ => Possibility::Sometimes,
    }
}

fn or(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Always, _) | (_, Possibility::Always) => Possibility::Always,
        (Possibility::Never, Possibility::Never) => Possibility::Never,
        _ => Possibility::Sometimes,
    }
}

fn negate(state: Possibility) -> Possibility {
    match state {
        Possibility::Never => Possibility::Always,
        Possibility::Always => Possibility::Never,
        Possibility::Sometimes => Possibility::Sometimes,
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{Possibility, attrs_possibility_in_test, meta_marks_test};

    #[test]
    fn cfg_attr_handles_conditional_and_test_profile_branches() -> Result<(), Box<dyn Error>> {
        let conditional: syn::Attribute =
            syn::parse_quote!(#[cfg_attr(feature = "disabled", cfg(any()))]);
        let test_filtered: syn::Attribute = syn::parse_quote!(#[cfg_attr(test, cfg(any()))]);
        assert_eq!(
            attrs_possibility_in_test(&[conditional])?,
            Possibility::Sometimes
        );
        assert_eq!(
            attrs_possibility_in_test(&[test_filtered])?,
            Possibility::Never
        );
        Ok(())
    }

    #[test]
    fn cfg_attr_test_marker_is_a_valid_attribute() -> Result<(), Box<dyn Error>> {
        let marker: syn::Attribute = syn::parse_quote!(#[cfg_attr(test, test)]);
        assert!(meta_marks_test(&marker.meta)?);
        Ok(())
    }
}
