//! Home-screen heat mapping (lane 14): every door's temperature, COMPUTED from
//! real store facts — never hardcoded. Heat is the app's whole notification
//! system, so there are no badges, counts, or red dots; a door's heat *is* its
//! state, on the continuous 0..1 dial the glyph renderer draws.
//!
//! The mapping is deliberately simple and honest — real facts only — and lands
//! each state in the neighborhood of the design gallery's reference `DIAL_HEAT`
//! feel values (bellows ≈ .97 when overdue, mercury ≈ .68 on a ripe thread,
//! adamas ≈ .85 as the mask last worn, azoth ≈ 0 unused, …).
//!
//! [`compute`] is a pure function of gathered inputs so it can be unit-tested
//! without a store; [`Store::home_heat`] gathers the facts and calls it.

use crate::domain::ThreadState;
use crate::error::CoreError;
use crate::store::Store;

const SECS_PER_DAY: u64 = 86_400;

/// Every door's heat on the 0..1 dial. `furnace` is the center forge core; the
/// other eight are the orbiting glyph doors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomeHeat {
    pub furnace: f32,
    pub bellows: f32,
    pub mercury: f32,
    pub grimoire: f32,
    pub tabula: f32,
    pub adamas: f32,
    pub philosophus: f32,
    pub solve: f32,
    pub azoth: f32,
}

/// The gathered store facts [`compute`] maps to heat. Kept separate from the
/// store so the mapping is a pure, exhaustively-testable function.
pub struct HeatInputs {
    /// Current clock (epoch seconds).
    pub now: u64,
    /// When the fire was last fed (epoch seconds), or `None` if never.
    pub last_tended_at: Option<u64>,
    /// Whether the fire was fed today.
    pub tended_today: bool,
    /// The states of the currently-open threads (volatile/condensing).
    pub open_thread_states: Vec<ThreadState>,
    /// The newest fixed grain's date (epoch seconds), or `None` if no salt yet.
    pub newest_grain_at: Option<u64>,
    /// How many Tabula passages remain unkindled (0..7).
    pub unkindled_passages: usize,
    /// Per-mask last-used timestamp `(mask_id, epoch_seconds)`.
    pub mask_usage: Vec<(String, u64)>,
}

fn days_between(now: u64, ts: u64) -> f32 {
    (now.saturating_sub(ts) / SECS_PER_DAY) as f32
}

/// Bellows = tending urgency. Fed today → at rest; otherwise urgency climbs
/// toward roaring as days pass (never fed reads as fully overdue).
fn bellows_heat(now: u64, last_tended_at: Option<u64>, tended_today: bool) -> f32 {
    if tended_today {
        return 0.30;
    }
    let d = last_tended_at
        .map(|ts| days_between(now, ts))
        .unwrap_or(3.0);
    (0.52 + 0.16 * d).min(0.97)
}

/// Furnace (center core) = held warmth. Warm the day it's fed, cooling over the
/// days after, never below the at-rest engraved floor.
fn furnace_heat(now: u64, last_tended_at: Option<u64>, tended_today: bool) -> f32 {
    if tended_today {
        return 0.85;
    }
    match last_tended_at {
        Some(ts) => (0.70 - 0.13 * days_between(now, ts)).max(0.30),
        None => 0.30,
    }
}

/// Mercury = the ripest open thread. A condensing thread is kindled; volatile
/// threads waiting are stirring; nothing open is at rest.
fn mercury_heat(states: &[ThreadState]) -> f32 {
    if states.iter().any(|s| *s == ThreadState::Condensing) {
        0.68
    } else if !states.is_empty() {
        0.50
    } else {
        0.30
    }
}

/// Grimoire = recent grain warmth. A grain fixed today is stirring→kindled and
/// cools over the next days back to the at-rest floor.
fn grimoire_heat(now: u64, newest_grain_at: Option<u64>) -> f32 {
    match newest_grain_at {
        Some(ts) => (0.60 - 0.07 * days_between(now, ts)).max(0.30),
        None => 0.30,
    }
}

/// Tabula = unkindled-passage pull: the more of the scroll is still dim, the
/// more it pulls. Fully kindled sits at rest.
fn tabula_heat(unkindled: usize) -> f32 {
    (0.30 + 0.03 * unkindled as f32).min(0.55)
}

/// Masks = affinity. The mask last worn runs molten and fades over a day; other
/// worn masks sit engraved, cooling toward the cold end the longer since worn; a
/// mask never worn is cold iron.
fn mask_heat(now: u64, mask: &str, usage: &[(String, u64)]) -> f32 {
    let Some(&(_, ts)) = usage.iter().find(|(m, _)| m == mask) else {
        return 0.0;
    };
    let most_recent = usage.iter().map(|(_, t)| *t).max().unwrap_or(ts);
    let d = days_between(now, ts);
    if ts == most_recent {
        (0.85 - 0.55 * d).max(0.30)
    } else {
        (0.36 - 0.04 * d).max(0.15)
    }
}

/// The pure heat mapping — real facts in, every door's 0..1 heat out.
pub fn compute(input: &HeatInputs) -> HomeHeat {
    HomeHeat {
        furnace: furnace_heat(input.now, input.last_tended_at, input.tended_today),
        bellows: bellows_heat(input.now, input.last_tended_at, input.tended_today),
        mercury: mercury_heat(&input.open_thread_states),
        grimoire: grimoire_heat(input.now, input.newest_grain_at),
        tabula: tabula_heat(input.unkindled_passages),
        adamas: mask_heat(input.now, "adamas", &input.mask_usage),
        philosophus: mask_heat(input.now, "philosophus", &input.mask_usage),
        solve: mask_heat(input.now, "solve", &input.mask_usage),
        azoth: mask_heat(input.now, "azoth", &input.mask_usage),
    }
}

impl Store {
    /// Computes the home screen's per-door heat from current store facts (lane
    /// 14). All clients inherit this — the mapping lives here, not in any UI.
    pub fn home_heat(&self) -> Result<HomeHeat, CoreError> {
        let now = self.now();
        let fire = self.fire_state()?;
        let open = self.open_threads()?;
        let newest_grain_at = self.list_realizations()?.last().map(|g| g.realization.date);
        let unkindled = self.tabula()?.iter().filter(|p| !p.kindled).count();

        let input = HeatInputs {
            now,
            last_tended_at: self.last_tended_at()?,
            tended_today: fire.tended_today,
            open_thread_states: open.into_iter().map(|t| t.state).collect(),
            newest_grain_at,
            unkindled_passages: unkindled,
            mask_usage: self.mask_usage()?,
        };
        Ok(compute(&input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = SECS_PER_DAY;
    const NOW: u64 = 1_000 * DAY; // an arbitrary "today"

    fn base() -> HeatInputs {
        HeatInputs {
            now: NOW,
            last_tended_at: None,
            tended_today: false,
            open_thread_states: vec![],
            newest_grain_at: None,
            unkindled_passages: 0,
            mask_usage: vec![],
        }
    }

    #[test]
    fn bellows_is_at_rest_when_tended_today_and_climbs_when_overdue() {
        let today = HeatInputs {
            tended_today: true,
            last_tended_at: Some(NOW),
            ..base()
        };
        assert_eq!(compute(&today).bellows, 0.30);

        let one_day = HeatInputs {
            last_tended_at: Some(NOW - DAY),
            ..base()
        };
        let three_days = HeatInputs {
            last_tended_at: Some(NOW - 3 * DAY),
            ..base()
        };
        assert!(
            compute(&one_day).bellows > 0.60,
            "a day overdue is kindling"
        );
        assert!(
            compute(&three_days).bellows >= 0.95,
            "three days overdue is roaring: {}",
            compute(&three_days).bellows
        );
        // never tended reads as fully overdue
        assert!(compute(&base()).bellows >= 0.95);
    }

    #[test]
    fn mercury_tracks_the_ripest_open_thread() {
        assert_eq!(compute(&base()).mercury, 0.30, "nothing open is at rest");
        let volatile = HeatInputs {
            open_thread_states: vec![ThreadState::Volatile],
            ..base()
        };
        assert_eq!(compute(&volatile).mercury, 0.50, "waiting is stirring");
        let condensing = HeatInputs {
            open_thread_states: vec![ThreadState::Volatile, ThreadState::Condensing],
            ..base()
        };
        assert_eq!(
            compute(&condensing).mercury,
            0.68,
            "a condensing thread is kindled"
        );
    }

    #[test]
    fn grimoire_warms_on_a_fresh_grain_and_cools_over_days() {
        let today = HeatInputs {
            newest_grain_at: Some(NOW),
            ..base()
        };
        let old = HeatInputs {
            newest_grain_at: Some(NOW - 6 * DAY),
            ..base()
        };
        assert!(compute(&today).grimoire > compute(&old).grimoire);
        assert_eq!(compute(&base()).grimoire, 0.30, "no salt yet is at rest");
        assert_eq!(
            compute(&old).grimoire,
            0.30,
            "an old grain has cooled to rest"
        );
    }

    #[test]
    fn tabula_pull_rises_with_unkindled_passages() {
        assert_eq!(compute(&base()).tabula, 0.30, "fully kindled is at rest");
        let some = HeatInputs {
            unkindled_passages: 4,
            ..base()
        };
        assert!(compute(&some).tabula > 0.30);
        let all = HeatInputs {
            unkindled_passages: 7,
            ..base()
        };
        assert!(compute(&all).tabula <= 0.55, "capped");
    }

    #[test]
    fn mask_affinity_hot_recent_cool_used_cold_unused() {
        // adamas worn today, philosophus worn 5 days ago, solve worn 10 days
        // ago, azoth never worn.
        let input = HeatInputs {
            mask_usage: vec![
                ("adamas".into(), NOW),
                ("philosophus".into(), NOW - 5 * DAY),
                ("solve".into(), NOW - 10 * DAY),
            ],
            ..base()
        };
        let h = compute(&input);
        assert!(
            h.adamas >= 0.85,
            "the mask last worn runs molten: {}",
            h.adamas
        );
        assert!(
            h.philosophus > h.solve && h.philosophus <= 0.30,
            "an earlier-worn mask is engraved, cooling: phil={} solve={}",
            h.philosophus,
            h.solve
        );
        assert!(
            h.solve >= 0.15,
            "a long-unworn mask cools to the floor, not below"
        );
        assert_eq!(h.azoth, 0.0, "a mask never worn is cold iron");
    }

    /// End-to-end over a real store: the gathering wiring (threads, grains,
    /// tabula, mask usage, tending) produces sane heats.
    #[test]
    fn store_home_heat_gathers_real_facts() {
        let store = Store::open_in_memory("heat").unwrap();

        // A fresh store: never tended → bellows overdue; nothing open → mercury
        // at rest; no salt → grimoire at rest; no session → every mask cold.
        let cold = store.home_heat().unwrap();
        assert!(
            cold.bellows >= 0.95,
            "never tended reads overdue: {}",
            cold.bellows
        );
        assert_eq!(cold.mercury, 0.30);
        assert_eq!(cold.grimoire, 0.30);
        assert_eq!(cold.adamas, 0.0, "no session yet → adamas cold iron");
        assert!(
            cold.tabula >= 0.30,
            "some passages unkindled → tabula pulls"
        );

        // Open a thread → mercury stirs; wear adamas in a session → adamas warms.
        store
            .open_thread("why does iron remember?", None, None)
            .unwrap();
        store.create_session(None, "adamas", "challenge").unwrap();
        let warm = store.home_heat().unwrap();
        assert_eq!(warm.mercury, 0.50, "an open volatile thread is stirring");
        assert!(
            warm.adamas >= 0.85,
            "the mask just worn runs molten: {}",
            warm.adamas
        );
        assert_eq!(warm.philosophus, 0.0, "an unworn mask stays cold iron");
    }

    #[test]
    fn the_mask_last_worn_fades_from_molten_to_engraved_over_a_day() {
        let today = HeatInputs {
            mask_usage: vec![("adamas".into(), NOW)],
            ..base()
        };
        let yesterday = HeatInputs {
            mask_usage: vec![("adamas".into(), NOW - DAY)],
            ..base()
        };
        assert!(compute(&today).adamas >= 0.85);
        assert_eq!(
            compute(&yesterday).adamas,
            0.30,
            "faded to engraved by the next day"
        );
    }
}
