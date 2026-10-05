#![no_std]
#![doc = include_str!("../README.md")]

use core::{
  fmt::{Debug, Display},
  ops::*,
};

/// Implements all of the bit operations for a boolean-like enum as well as conversions to/from `bool`.
pub trait BooleanEnum:
  Into<bool>
  + From<bool>
  + Default
  + Copy
  + Display
  + Debug
  + Eq
  + Ord
  + core::hash::Hash
  + Deref<Target = bool>
  + Not<Output = Self>
  + BitAnd<Output = Self>
  + BitAndAssign<Self>
  + BitOr<Output = Self>
  + BitOrAssign<Self>
  + BitXor<Output = Self>
  + BitXorAssign<Self>
  + Send
  + Sync
{
  #[inline]
  fn as_bool(self) -> bool {
    *self
  }
}

/// Generates a boolean-like enum that implements [`BooleanEnum`].
///
/// The first argument is for the visibility, which can be empty to signify a private enum.
/// The second argument is the ident for the enum.
/// The third (optional) argument is for the documentation.
///
/// # Example
///
/// ```
/// use bool_enum::{bool_enum};
///
/// bool_enum!(NoVis);
/// bool_enum!(pub(crate) PubCrate);
/// bool_enum!(pub Public, doc = "Some documentation here...");
///
/// fn main() {
///     let yes = Public::Yes;
///     let no = Public::No;
///
///     if *no { panic!("Default/No should deref to false") }
///     if !*yes { panic!("Yes should deref to true") }
///
///     assert_eq!(!no, yes, "!No should be Yes");
///     assert_eq!(!yes, no, "!Yes should be No");
///
///     assert_eq!(yes & yes, yes, "Yes & Yes should be Yes");
///     assert_eq!(yes & no, no, "Yes & No should be No");
///
///     let mut val = yes;
///     val &= no;
///     assert_eq!(val, no, "&= logic failed");
///
///     assert_eq!(no | yes, yes, "No | Yes should be Yes");
///     assert_eq!(no | no, no, "No | No should be No");
///
///     let mut val = no;
///     val |= yes;
///     assert_eq!(val, yes, "|= logic failed");
///
///     assert_eq!(yes ^ no, yes, "Yes ^ No should be Yes");
///     assert_eq!(yes ^ yes, no, "Yes ^ Yes should be No");
///
///     let mut val = yes;
///     val ^= yes;
///     assert_eq!(val, no, "^= logic failed (toggle off)");
///
///     val ^= yes;
///     assert_eq!(val, yes, "^= logic failed (toggle on)");
/// }
/// ```
#[macro_export]
macro_rules! bool_enum {
  ($vis:vis $ident:ident $(, doc = $doc:literal)?) => {

    $(#[doc = $doc])?
    #[derive(::core::fmt::Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ::core::hash::Hash)]
    #[repr(u8)]
    $vis enum $ident {
      Yes = 1,
      No = 0
    }

    impl $crate::BooleanEnum for $ident {}

    impl ::core::ops::Not for $ident {
      type Output = Self;
      #[inline]
      fn not(self) -> Self::Output {
        match self {
          Self::Yes => Self::No,
          Self::No => Self::Yes,
        }
      }
    }

    impl ::core::ops::BitAnd for $ident {
      type Output = Self;
      #[inline]
      fn bitand(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
          (Self::Yes, Self::Yes) => Self::Yes,
          _ => Self::No,
        }
      }
    }

    impl ::core::ops::BitAndAssign for $ident {
      #[inline]
      fn bitand_assign(&mut self, rhs: Self) {
        if let Self::Yes = self {
          if let Self::No = rhs {
            *self = Self::No;
          }
        }
      }
    }

    impl ::core::ops::BitOr for $ident {
      type Output = Self;
      #[inline]
      fn bitor(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
          (Self::No, Self::No) => Self::No,
          _ => Self::Yes,
        }
      }
    }

    impl ::core::ops::BitOrAssign for $ident {
      #[inline]
      fn bitor_assign(&mut self, rhs: Self) {
        if let Self::No = self {
          if let Self::Yes = rhs {
            *self = Self::Yes;
          }
        }
      }
    }

    impl ::core::ops::BitXor for $ident {
      type Output = Self;
      #[inline]
      fn bitxor(self, rhs: Self) -> Self::Output {
        if self != rhs {
          Self::Yes
        } else {
          Self::No
        }
      }
    }

    impl ::core::ops::BitXorAssign for $ident {
      #[inline]
      fn bitxor_assign(&mut self, rhs: Self) {
        if *self != rhs {
          *self = Self::Yes;
        } else {
          *self = Self::No;
        }
      }
    }

    impl Default for $ident {
      #[inline]
      fn default() -> Self {
        Self::No
      }
    }

    impl From<$ident> for bool {
      #[inline]
      fn from(value: $ident) -> Self {
        match value {
          $ident::Yes => true,
          $ident::No => false,
        }
      }
    }

    impl From<bool> for $ident {
      #[inline]
      fn from(value: bool) -> Self {
        if value { Self::Yes } else { Self::No }
      }
    }

    impl ::core::ops::Deref for $ident {
      type Target = bool;

      #[inline]
      fn deref(&self) -> &Self::Target {
        match self {
          $ident::Yes => &true,
          $ident::No => &false,
        }
      }
    }

    impl ::core::fmt::Display for $ident {
      fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        ::core::write!(f, "{}", *self)
      }
    }
  };
}
