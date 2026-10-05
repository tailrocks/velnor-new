This crate exports the [`bool_enum`](crate::bool_enum) macro for creating boolean-like enums, which implement the [`Not`](core::ops::Not) operator as well as the bitwise operators and Into/From `bool`. These can be useful for tracking boolean-like logic while maintaining more clarity and type safety across code boundaries.

It also exports the [`BooleanEnum`](crate::BooleanEnum) trait for usage with generics.

## Usage

Call the macro with visibility (optional) and the ident of the enum to create, followed by an optional string for documentation.

```rust
use bool_enum::{bool_enum};

bool_enum!(NoVis);
bool_enum!(pub(crate) PubCrate);
bool_enum!(pub Public, doc = "Some documentation here...");

fn main() {
    let yes = Public::Yes;
    let no = Public::No;

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
```
