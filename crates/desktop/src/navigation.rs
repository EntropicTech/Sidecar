//! Runtime navigation stays small; retired views are fixture-only compatibility tools.
pub const TABS: [(usize, &str); 3] = [
    (1, "Applications"),
    (0, "GPU Activity"),
    (2, "Launch Settings"),
];

pub fn normalize(view: usize, fixtures: bool) -> usize {
    match view {
        0..=2 => view,
        3 | 4 if fixtures => view,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_or_invalid_views_cannot_become_runtime_pages() {
        for view in [3, 4, 5, usize::MAX] {
            assert_eq!(normalize(view, false), 1);
        }
        for view in 0..=2 {
            assert_eq!(normalize(view, false), view);
        }
        assert_eq!(normalize(3, true), 3);
        assert_eq!(normalize(4, true), 4);
        assert_eq!(normalize(usize::MAX, true), 1);
    }
}
