use rusuh::proxy::balancer::{Balancer, Strategy};

#[test]
fn round_robin_distributes_evenly() {
    let b = Balancer::new(Strategy::RoundRobin, 3);
    let candidates = vec![0, 1, 2];

    let mut picks = [0u32; 3];
    for _ in 0..300 {
        let idx = b.pick(&candidates);
        picks[idx] += 1;
    }

    assert_eq!(picks[0], 100);
    assert_eq!(picks[1], 100);
    assert_eq!(picks[2], 100);
}

#[test]
fn round_robin_cycles_through_subset() {
    let b = Balancer::new(Strategy::RoundRobin, 5);
    let candidates = vec![1, 3];

    let first = b.pick(&candidates);
    let second = b.pick(&candidates);
    let third = b.pick(&candidates);

    assert_eq!(first, 1);
    assert_eq!(second, 3);
    assert_eq!(third, 1);
}

#[test]
fn fill_first_always_picks_first() {
    let b = Balancer::new(Strategy::FillFirst, 3);
    let candidates = vec![0, 1, 2];

    for _ in 0..100 {
        assert_eq!(b.pick(&candidates), 0);
    }
}

#[test]
fn request_counters_track() {
    let b = Balancer::new(Strategy::RoundRobin, 3);
    let candidates = vec![0, 1, 2];

    for _ in 0..9 {
        b.pick(&candidates);
    }

    assert_eq!(b.request_count(0), 3);
    assert_eq!(b.request_count(1), 3);
    assert_eq!(b.request_count(2), 3);
    assert_eq!(b.total_requests(), 9);
}

#[test]
fn single_candidate_always_returns_it() {
    let b = Balancer::new(Strategy::RoundRobin, 5);
    for _ in 0..10 {
        assert_eq!(b.pick(&[3]), 3);
    }
}

#[test]
fn empty_candidates_returns_zero() {
    let b = Balancer::new(Strategy::RoundRobin, 3);
    assert_eq!(b.pick(&[]), 0);
}

#[test]
fn strategy_from_str_parsing() {
    assert_eq!(Strategy::parse("round-robin"), Strategy::RoundRobin);
    assert_eq!(Strategy::parse("fill-first"), Strategy::FillFirst);
    assert_eq!(Strategy::parse("fill_first"), Strategy::FillFirst);
    assert_eq!(Strategy::parse("fillfirst"), Strategy::FillFirst);
    assert_eq!(Strategy::parse("anything-else"), Strategy::RoundRobin);
    assert_eq!(Strategy::parse(""), Strategy::RoundRobin);
}

#[test]
fn fill_first_counter_tracks_first_only() {
    let b = Balancer::new(Strategy::FillFirst, 3);
    let candidates = vec![0, 1, 2];

    for _ in 0..50 {
        b.pick(&candidates);
    }

    assert_eq!(b.request_count(0), 50);
    assert_eq!(b.request_count(1), 0);
    assert_eq!(b.request_count(2), 0);
}
