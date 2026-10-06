use super::gate_line;
use velnor_runner_host::Reconcile;

#[test]
fn line_counts_and_hides_docker_ids() {
    let hold = Reconcile::Hold {
        adopt: vec!["0123456789abcdef".to_owned()],
        occupied: 2,
    };
    let line = gate_line(&hold);
    assert_eq!(line, "hold occupied=2 adopt=1");
    assert!(!line.contains("0123456789abcdef"));
    let open = Reconcile::Advertise { occupied: 0 };
    assert_eq!(gate_line(&open), "advertise occupied=0");
}
