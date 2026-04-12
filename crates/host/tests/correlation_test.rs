use omnifs_host::runtime::correlation::CorrelationTracker;

#[test]
fn test_allocate_unique_ids() {
    let tracker = CorrelationTracker::new();
    let id1 = tracker.allocate();
    let id2 = tracker.allocate();
    assert_ne!(id1, id2);
}

#[test]
fn test_track_and_resolve_pending() {
    let tracker = CorrelationTracker::new();
    let id = tracker.allocate();
    tracker.mark_pending(id, "list_entries".to_string());
    assert!(tracker.is_pending(id));
    tracker.resolve(id);
    assert!(!tracker.is_pending(id));
}

#[test]
fn test_cancel_removes_pending() {
    let tracker = CorrelationTracker::new();
    let id = tracker.allocate();
    tracker.mark_pending(id, "lookup".to_string());
    tracker.cancel(id);
    assert!(!tracker.is_pending(id));
}

#[test]
fn test_pending_count() {
    let tracker = CorrelationTracker::new();
    let id1 = tracker.allocate();
    let id2 = tracker.allocate();
    tracker.mark_pending(id1, "a".to_string());
    tracker.mark_pending(id2, "b".to_string());
    assert_eq!(tracker.pending_count(), 2);
    tracker.resolve(id1);
    assert_eq!(tracker.pending_count(), 1);
}
