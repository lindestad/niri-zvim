pub fn assigned_window_id(
    client_id: u16,
    direct_window_id: Option<u64>,
    client_ids: &[u16],
    window_ids: &[u64],
) -> Option<u64> {
    direct_window_id.or_else(|| client_window_id(client_id, client_ids, window_ids))
}

fn client_window_id(client_id: u16, client_ids: &[u16], window_ids: &[u64]) -> Option<u64> {
    if client_ids.len() != window_ids.len() {
        return None;
    }
    client_ids
        .iter()
        .position(|candidate| *candidate == client_id)
        .and_then(|position| window_ids.get(position))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_binding_does_not_depend_on_other_zellij_clients() {
        assert_eq!(
            assigned_window_id(7, Some(42), &[1, 7, 9], &[100]),
            Some(42)
        );
    }

    #[test]
    fn local_binding_still_requires_one_window_per_client() {
        assert_eq!(assigned_window_id(7, None, &[1, 7], &[41, 42]), Some(42));
        assert_eq!(assigned_window_id(7, None, &[1, 7], &[42]), None);
    }
}
