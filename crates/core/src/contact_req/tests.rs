use super::*;

#[test]
fn errors_map_by_kind_not_text() {
    assert_eq!(AddError::from(Error::Timeout), AddError::Unreachable);
    assert_eq!(AddError::from(Error::Net("x".into())), AddError::Unreachable);
    assert_eq!(AddError::from(Error::NotStarted), AddError::Offline);
    assert_eq!(AddError::from(Error::Rejected(crate::BLOCKED.into())), AddError::Blocked);
    assert_eq!(AddError::from(Error::Rejected("not accepted".into())), AddError::Refused);
    assert!(matches!(AddError::from(Error::Protocol("x".into())), AddError::Failed(_)));
}
