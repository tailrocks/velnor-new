use mbx_synchronous_registry_fixture::decimal_length;

#[test]
fn zero_has_one_digit() {
    assert_eq!(decimal_length(0), 1);
}

#[test]
fn single_digit_has_one_digit() {
    assert_eq!(decimal_length(9), 1);
}

#[test]
fn first_two_digit_value_has_two_digits() {
    assert_eq!(decimal_length(10), 2);
}

#[test]
fn last_two_digit_value_has_two_digits() {
    assert_eq!(decimal_length(99), 2);
}

#[test]
fn first_three_digit_value_has_three_digits() {
    assert_eq!(decimal_length(100), 3);
}

#[test]
fn last_three_digit_value_has_three_digits() {
    assert_eq!(decimal_length(999), 3);
}

#[test]
fn first_four_digit_value_has_four_digits() {
    assert_eq!(decimal_length(1_000), 4);
}

#[test]
fn nine_digit_value_has_nine_digits() {
    assert_eq!(decimal_length(123_456_789), 9);
}

#[test]
fn u32_max_has_ten_digits() {
    assert_eq!(decimal_length(u64::from(u32::MAX)), 10);
}

#[test]
fn u64_max_has_twenty_digits() {
    assert_eq!(decimal_length(u64::MAX), 20);
}
