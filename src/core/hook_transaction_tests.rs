use super::hook_transaction::install_all_or_rollback;

#[test]
fn successful_transaction_installs_every_step_without_rollback() {
    let mut installed = Vec::new();
    let mut rolled_back = Vec::new();
    assert!(install_all_or_rollback(
        5,
        |index| {
            installed.push(index);
            true
        },
        |index| rolled_back.push(index),
    ));
    assert_eq!(installed, vec![0, 1, 2, 3, 4]);
    assert!(rolled_back.is_empty());
}

#[test]
fn every_failure_position_rolls_back_prior_steps_in_reverse() {
    for failure in 0..5 {
        let mut installed = Vec::new();
        let mut rolled_back = Vec::new();
        assert!(!install_all_or_rollback(
            5,
            |index| {
                installed.push(index);
                index != failure
            },
            |index| rolled_back.push(index),
        ));
        assert_eq!(installed, (0..=failure).collect::<Vec<_>>());
        assert_eq!(rolled_back, (0..failure).rev().collect::<Vec<_>>());
    }
}
