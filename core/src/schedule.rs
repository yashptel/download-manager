//! The scheduler: starts the downloads that wait in the list (paused, as
//! "Download Later" leaves them) at a time of day, and pauses them again at
//! another, on the chosen weekdays.

use crate::{Shared, tasks};
use chrono::{Datelike, Local, NaiveTime, Timelike};
use std::sync::Arc;
use std::time::Duration;

pub async fn run(shared: Arc<Shared>) {
    let mut shutdown = shared.shutdown.subscribe();
    // The last minute acted on, so a minute is acted on once.
    let mut last: Option<(i32, u32, u32)> = None;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(15)) => {}
            _ = crate::stopped(&mut shutdown) => return,
        }
        let schedule = shared.settings.lock().unwrap().schedule.clone();
        let now = Local::now();
        let minute = (now.year(), now.ordinal(), now.hour() * 60 + now.minute());
        if !schedule.enabled || last == Some(minute) {
            continue;
        }
        let today = now.weekday().num_days_from_sunday() as u8;
        if !schedule.days.contains(&today) {
            continue;
        }
        let at = |time: &str| NaiveTime::parse_from_str(time.trim(), "%H:%M").ok().map(|t| t.hour() * 60 + t.minute());
        let action = if at(&schedule.start) == Some(minute.2) {
            tasks::resume_all(&shared).await
        } else if at(&schedule.stop) == Some(minute.2) {
            tasks::pause_all(&shared).await
        } else {
            continue;
        };
        last = Some(minute);
        if let Err(error) = action {
            shared.listener.error(format!("The scheduler could not act: {error}"));
        }
    }
}

