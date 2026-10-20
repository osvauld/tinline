use super::find_card;

#[test]
fn finds_a_card_in_surrounding_text() {
    let card = "OSVC2:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U";
    assert_eq!(find_card(card).as_deref(), Some(card));
    assert_eq!(find_card(&format!("Add me on Tinline:\n{card}.")).as_deref(), Some(card));
    assert_eq!(find_card(&format!("\"{card}\" thanks")).as_deref(), Some(card));
    assert_eq!(find_card(&format!("osvc2:{}", card.get(6..).unwrap())).as_deref(), Some(&*format!("osvc2:{}", card.get(6..).unwrap())));
    assert_eq!(find_card("hello there"), None);
    assert_eq!(find_card("OSVC2:short"), None);
}
