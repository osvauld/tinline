use super::*;

#[test]
fn defaults() {
    let a = parse_args(&[]).unwrap();
    assert_eq!(a, HostArgs { name: "Tinline host".into(), accept: vec![], join: vec![] });
}

#[test]
fn repeatable_flags() {
    let a = parse_args(&["--accept", "g1", "--name", "H", "--accept", "g2", "--join", "L"]).unwrap();
    assert_eq!(a.name, "H");
    assert_eq!(a.accept, ["g1", "g2"]);
    assert_eq!(a.join, ["L"]);
}

#[test]
fn rejects_bad_input() {
    assert!(parse_args(&["--accept"]).is_err());
    assert!(parse_args(&["--bogus", "x"]).is_err());
}
