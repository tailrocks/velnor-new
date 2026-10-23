use super::civil_from_days;

#[test]
fn civil_date_conversion_covers_epoch_leap_day_and_year_transition() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    assert_eq!(civil_from_days(20_089), (2025, 1, 1));
}
