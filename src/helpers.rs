pub fn windowed<'b, 'a: 'b, T>(
    collection: &'a [T],
    size: usize,
) -> impl Iterator<Item = &'a [T]> + 'b {
    assert!(size > 0);
    assert!(collection.len() >= size);
    (0..collection.len() - size + 1).map(move |start| &collection[start..(start + size)])
}

#[cfg(test)]
mod test_helpers {
    use itertools::Itertools;

    use super::*;

    #[test]
    fn test_windowed_helper() {
        assert_eq!(
            windowed(&[1, 2, 3], 1).collect_vec(),
            vec![vec![1], vec![2], vec![3]]
        );
        assert_eq!(
            windowed(&[1, 2, 3], 2).collect_vec(),
            vec![vec![1, 2], vec![2, 3]]
        );
        assert_eq!(windowed(&[1, 2, 3], 3).collect_vec(), vec![vec![1, 2, 3]]);
    }
}
