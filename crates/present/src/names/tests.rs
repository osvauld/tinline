use super::*;

#[test]
fn first_name_table() {
    for (input, want) in [
        ("Ann Lee", "Ann"),
        ("  Ann   Lee ", "Ann"),
        ("Ann", "Ann"),
        ("Ann\tLee", "Ann"),
        ("", ""),
        ("   ", "   "),
    ] {
        assert_eq!(first_name(input.into()), want, "{input:?}");
    }
}

#[test]
fn initials_table() {
    for (input, want) in [
        ("Lena (work)", "LW"),
        ("Ann", "A"),
        ("ann lee", "AL"),
        ("Ann Marie Lee", "AL"),
        ("  ", "?"),
        ("", "?"),
        ("(( ))", "?"),
        ("7 Eleven", "7E"),
        ("\u{e9}mile zola", "\u{c9}Z"),
        ("\u{df}", "SS"),
    ] {
        assert_eq!(initials(input.into()), want, "{input:?}");
    }
}

#[test]
fn display_name_table() {
    let s = |x: &str| Some(x.to_string());
    for (alias, name, want) in [
        (s("Mum"), "Asha", "Mum"),
        (None, "Asha", "Asha"),
        (s("  "), "Asha", "Asha"),
        (s(""), "", "Unnamed"),
        (None, "  ", "Unnamed"),
    ] {
        assert_eq!(
            display_name(alias.clone(), name.into()),
            want,
            "{alias:?} {name:?}"
        );
    }
}
