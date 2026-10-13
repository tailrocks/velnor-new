use crate::{DisconnectEffect, SetOwnership, disconnect_effects};

#[test]
fn disconnect_deletes_only_recorded_ownership() {
    assert_eq!(
        disconnect_effects(SetOwnership::Adopted, true),
        vec![DisconnectEffect::Drain]
    );
    assert_eq!(
        disconnect_effects(SetOwnership::Adopted, false),
        Vec::<DisconnectEffect>::new()
    );
    assert_eq!(
        disconnect_effects(SetOwnership::Created, true),
        vec![DisconnectEffect::Drain, DisconnectEffect::DeleteSet]
    );
    assert_eq!(
        disconnect_effects(SetOwnership::Created, false),
        vec![DisconnectEffect::DeleteSet]
    );
    let adopted = disconnect_effects(SetOwnership::Adopted, true);
    assert!(!adopted.contains(&DisconnectEffect::DeleteSet));
}
