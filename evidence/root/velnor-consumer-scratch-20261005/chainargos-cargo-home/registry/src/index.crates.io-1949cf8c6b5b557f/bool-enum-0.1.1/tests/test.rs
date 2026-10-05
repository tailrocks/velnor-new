use bool_enum::bool_enum;

bool_enum!(NoVis);
bool_enum!(pub(crate) PubCrate);
bool_enum!(pub Public);

mod pub_super {
  use super::*;
  bool_enum!(pub(super) PubSuper);
}
use pub_super::*;

macro_rules! test_bool_enum {
  ($($ident:ident),*) => {
    paste::paste! {
      $(
        #[test]
        fn [< $ident:snake >]() {
          let yes = $ident::Yes;
          let no = $ident::No;

          if *no { panic!("Default/No should deref to false") }
          if !*yes { panic!("Yes should deref to true") }

          assert_eq!(!no, yes, "!No should be Yes");
          assert_eq!(!yes, no, "!Yes should be No");

          assert_eq!(yes & yes, yes, "Yes & Yes should be Yes");
          assert_eq!(yes & no, no, "Yes & No should be No");

          let mut val = yes;
          val &= no;
          assert_eq!(val, no, "&= logic failed");

          assert_eq!(no | yes, yes, "No | Yes should be Yes");
          assert_eq!(no | no, no, "No | No should be No");

          let mut val = no;
          val |= yes;
          assert_eq!(val, yes, "|= logic failed");

          assert_eq!(yes ^ no, yes, "Yes ^ No should be Yes");
          assert_eq!(yes ^ yes, no, "Yes ^ Yes should be No");

          let mut val = yes;
          val ^= yes;
          assert_eq!(val, no, "^= logic failed (toggle off)");

          val ^= yes;
          assert_eq!(val, yes, "^= logic failed (toggle on)");
        }
      )*
    }
  };
}

test_bool_enum!(NoVis, PubCrate, PubSuper, Public);
