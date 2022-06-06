use crate::compressor::Level;

#[test]
fn new_produces_empty_level() {
    let l = Level::new(15);
    assert_eq!(l.max_length, 15);
    assert_eq!(l.current_chain_length, 0);
    assert_eq!(l.head, None);
}

#[test]
fn update_adds_to_non_full_level() {
    let mut l = Level::new(10);
    l.update(7, true);
    assert_eq!(l.max_length, 10);
    assert_eq!(l.current_chain_length, 1);
    assert_eq!(l.head, Some(7));
}

#[test]
#[should_panic(expected = "Tried to add to an already full level")]
fn update_panics_if_adding_and_too_full() {
    let mut l = Level::new(5);
    l.update(1, true);
    l.update(2, true);
    l.update(3, true);
    l.update(4, true);
    l.update(5, true);
    l.update(6, true);
}

#[test]
fn update_resets_level_correctly() {
    let mut l = Level::new(5);
    l.update(1, true);
    l.update(2, true);
    l.update(3, true);
    l.update(4, true);
    l.update(5, true);
    l.update(6, false);
    assert_eq!(l.max_length, 5);
    assert_eq!(l.current_chain_length, 1);
    assert_eq!(l.head, Some(6));
}

#[test]
fn get_head_returns_head() {
    let mut l = Level::new(5);
    assert_eq!(l.get_head(), None);
    l.update(23, true);
    assert_eq!(l.get_head(), Some(23));
}

#[test]
fn has_space_returns_true_if_empty() {
    let l = Level::new(15);
    assert!(l.has_space());
}

#[test]
fn has_space_returns_true_if_part_full() {
    let mut l = Level::new(15);
    l.update(12, true);
    l.update(234, true);
    l.update(1, true);
    l.update(143, true);
    l.update(15, true);
    assert!(l.has_space());
}

#[test]
fn has_space_returns_false_if_full() {
    let mut l = Level::new(5);
    l.update(1, true);
    l.update(2, true);
    l.update(3, true);
    l.update(4, true);
    l.update(5, true);
    assert!(!l.has_space());
}
