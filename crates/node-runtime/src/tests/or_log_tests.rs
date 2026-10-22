use crate::*;

#[test]
fn or_log_keeps_values_and_turns_errors_into_none() {
    assert_eq!(Ok::<_, Error>(3).or_log("x"), Some(3));
    assert_eq!(Err::<u8, _>(Error::Locked).or_log("x"), None);
    assert_eq!(
        Err::<Vec<u8>, _>(Error::Locked).or_log_default("x"),
        Vec::<u8>::new()
    );
    assert_eq!(Ok::<_, Error>(vec![1]).or_log_default("x"), vec![1]);
}

#[test]
fn or_log_flat_separates_absent_from_failed_only_in_the_log() {
    assert_eq!(Ok::<_, Error>(Some(1)).or_log_flat("x"), Some(1));
    assert_eq!(Ok::<Option<u8>, Error>(None).or_log_flat("x"), None);
    assert_eq!(Err::<Option<u8>, _>(Error::Locked).or_log_flat("x"), None);
}
