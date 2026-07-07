//! Session state machine: thread lifecycle transitions (the DAG behind
//! `Store::set_thread_state`) plus the `close_session`/`abandon_session`
//! entry points that tie a session's end to tending/wisdom accounting.
//!
//! DAG: `volatile -> condensing -> fixed` is the happy path; `condensing ->
//! volatile` is the refusal path (work on a thread stalls, it's returned to
//! the pool); any non-evaporated state can move to `evaporated` (archived);
//! `fixed` is otherwise terminal — once a thread is fixed it doesn't move
//! back into the working pool.

use crate::domain::ThreadState;
use crate::error::CoreError;
use crate::store::Store;

impl ThreadState {
    /// Is moving from `self` to `target` a legal transition?
    pub fn can_transition_to(self, target: ThreadState) -> bool {
        use ThreadState::*;
        match (self, target) {
            // any non-terminal move to evaporated is an archive; allowed
            // from every state, including a no-op evaporated -> evaporated.
            (_, Evaporated) => true,
            (Volatile, Condensing) => true,
            (Condensing, Fixed) => true,
            (Condensing, Volatile) => true,
            _ => false,
        }
    }
}

/// Days since the Unix epoch -> proleptic Gregorian (y, m, d). Howard
/// Hinnant's `civil_from_days` algorithm (public domain), used so we don't
/// need a date/time crate dependency for a "which UTC day is this" lookup.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Inverse of `civil_from_days`: (y, m, d) -> days since the Unix epoch.
/// Test-only: production code only ever needs epoch -> day (`today_utc`);
/// this direction exists so tests can build a deterministic clock anchored
/// to a specific calendar day.
#[cfg(test)]
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (m as i64 + if m > 2 { -3 } else { 9 }) + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The UTC calendar day (`YYYY-MM-DD`) containing `epoch_secs`.
pub fn today_utc(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parses a `YYYY-MM-DD` string back to midnight-UTC epoch seconds. Used by
/// tests to build a deterministic clock anchored to a specific day.
#[cfg(test)]
fn epoch_secs_for_day(date: &str) -> u64 {
    let parts: Vec<i64> = date.split('-').map(|p| p.parse().unwrap()).collect();
    let days = days_from_civil(parts[0], parts[1] as u32, parts[2] as u32);
    (days * 86_400) as u64
}

/// Closes a session: marks it `closed`, then records `minutes`/`thread_ids`
/// against today's tending row (upsert-add — see `record_tending`). This is
/// the only place `wisdom_days` moves: the first close of a new UTC day adds
/// a tending row (and so a wisdom day); later closes the same day merge into
/// the existing row.
pub fn close_session(
    store: &Store,
    session_id: &str,
    minutes: u32,
    thread_ids: &[String],
) -> Result<(), CoreError> {
    store.mark_session_closed(session_id)?;
    let day = today_utc(store.now());
    store.record_tending(&day, minutes, thread_ids)?;
    Ok(())
}

/// Abandons a session (interrupted mid-way): marks it `abandoned` and, per
/// the product spec's error-handling rule, returns its thread (if any) to
/// `volatile` so it re-enters the working pool instead of staying stuck
/// mid-transition.
pub fn abandon_session(store: &Store, session_id: &str) -> Result<(), CoreError> {
    let session = store.mark_session_abandoned(session_id)?;
    if let Some(thread_id) = session.thread_id {
        store.set_thread_state(&thread_id, ThreadState::Volatile)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Clock;

    fn fixed_day(date: &str, offset_secs: u64) -> Clock {
        let base = epoch_secs_for_day(date);
        Arc::new(move || base + offset_secs)
    }

    #[test]
    fn thread_transitions_are_constrained() {
        // volatile -> condensing -> fixed  (legal); fixed -> * illegal; * -> evaporated legal
        assert!(ThreadState::Volatile.can_transition_to(ThreadState::Condensing));
        assert!(ThreadState::Condensing.can_transition_to(ThreadState::Fixed));
        assert!(!ThreadState::Fixed.can_transition_to(ThreadState::Volatile));
        assert!(ThreadState::Condensing.can_transition_to(ThreadState::Volatile)); // refusal returns it
        assert!(ThreadState::Volatile.can_transition_to(ThreadState::Evaporated));
    }

    #[test]
    fn closing_first_session_of_day_adds_one_tending_row_and_bumps_wisdom() {
        let store = Store::open_in_memory("dev")
            .unwrap()
            .with_clock(fixed_day("2026-07-06", 0));
        let s = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        assert_eq!(store.wisdom_days().unwrap(), 0);
        close_session(&store, &s.id, 7 /*minutes*/, &[]).unwrap();
        assert_eq!(store.wisdom_days().unwrap(), 1);
        // a SECOND session same day adds minutes but NOT a new wisdom day
        let s2 = store.create_session(None, "adamas", "challenge").unwrap();
        close_session(&store, &s2.id, 5, &[]).unwrap();
        assert_eq!(
            store.wisdom_days().unwrap(),
            1,
            "wisdom counts days, not sessions"
        );
    }

    #[test]
    fn closing_a_session_the_next_day_adds_a_second_wisdom_day() {
        let store = Store::open_in_memory("dev")
            .unwrap()
            .with_clock(fixed_day("2026-07-06", 0));
        let s = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        close_session(&store, &s.id, 7, &[]).unwrap();
        assert_eq!(store.wisdom_days().unwrap(), 1);

        let store = store.with_clock(fixed_day("2026-07-07", 0));
        let s2 = store
            .create_session(None, "philosophus", "explain")
            .unwrap();
        close_session(&store, &s2.id, 5, &[]).unwrap();
        assert_eq!(store.wisdom_days().unwrap(), 2);
    }

    #[test]
    fn close_session_marks_state_closed_with_ended_at() {
        let store = Store::open_in_memory("dev")
            .unwrap()
            .with_clock(fixed_day("2026-07-06", 100));
        let s = store.create_session(None, "mystagogue", "trace").unwrap();
        close_session(&store, &s.id, 3, &[]).unwrap();
        let reloaded = store.get_session(&s.id).unwrap();
        assert_eq!(reloaded.state, "closed");
        assert_eq!(
            reloaded.ended_at,
            Some(epoch_secs_for_day("2026-07-06") + 100)
        );
    }

    #[test]
    fn abandon_session_returns_thread_to_volatile() {
        let store = Store::open_in_memory("dev").unwrap();
        let thread = store.open_thread("why?", None, None).unwrap();
        store
            .set_thread_state(&thread.id, ThreadState::Condensing)
            .unwrap();
        let s = store
            .create_session(Some(&thread.id), "mystagogue", "explain")
            .unwrap();
        abandon_session(&store, &s.id).unwrap();
        let reloaded = store.get_thread(&thread.id).unwrap();
        assert_eq!(reloaded.state, ThreadState::Volatile);
    }

    #[test]
    fn abandon_session_with_no_thread_is_fine() {
        let store = Store::open_in_memory("dev").unwrap();
        let s = store.create_session(None, "mystagogue", "trace").unwrap();
        abandon_session(&store, &s.id).unwrap();
    }

    #[test]
    fn today_utc_round_trips_through_civil_conversion() {
        assert_eq!(today_utc(epoch_secs_for_day("2026-07-06")), "2026-07-06");
        assert_eq!(today_utc(epoch_secs_for_day("2026-01-01")), "2026-01-01");
        assert_eq!(today_utc(epoch_secs_for_day("2000-02-29")), "2000-02-29"); // leap day
    }
}
