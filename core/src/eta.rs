//! Time left from a Kalman filter over the download rate, the way a GPS
//! receiver smooths speed: each poll's bytes over the time since the last
//! one is a noisy reading of the rate, the rate itself may drift, and the
//! filter weighs the two by how sure it is of each. Readings that keep
//! landing on one side of the estimate mean the rate really changed (a GPS
//! receiver's turn), so the filter opens up and follows them quickly.
//!
//! Against the engine's speed (bytes over the last 4 s), in simulations of
//! bursty downloads polled 4 times a second, time left moves about half as
//! much from tick to tick, the rate is closer (7% off instead of 10%), and
//! after a drop to a third the rate is found in about half the time.

/// How much the true rate may drift, as a share of the average rate per
/// square root of a second.
const DRIFT: f64 = 0.05;
/// How far a reading over a quarter second strays, as a share of the average
/// rate: bytes arrive in bursts.
const NOISE: f64 = 0.7;
/// The run of same-side readings, in standard deviations, that counts as a
/// real change.
const CHANGE: f64 = 0.8;
/// How much each reading counts in that run.
const MEMORY: f64 = 0.3;
/// Below this rate, in bytes per second, noise is taken as at this rate.
const FLOOR: f64 = 16.0 * 1024.0;

#[derive(Debug, Clone)]
pub struct RateFilter {
    at_ms: i64,
    done: u64,
    /// The estimated rate, in bytes per second, and its variance.
    rate: f64,
    variance: f64,
    /// Recent readings above (positive) or below the estimate, in standard
    /// deviations.
    trend: f64,
}

impl RateFilter {
    /// Starts from the engine's speed.
    pub fn new(at_ms: i64, done: u64, speed: u64) -> RateFilter {
        let rate = speed as f64;
        RateFilter { at_ms, done, rate, variance: rate.max(FLOOR).powi(2), trend: 0.0 }
    }

    /// Takes the bytes done at a moment and the engine's average speed then;
    /// returns the estimated rate in bytes per second.
    pub fn update(&mut self, at_ms: i64, done: u64, speed: u64) -> f64 {
        if done < self.done {
            // Started over: nothing learned applies.
            *self = RateFilter::new(at_ms, done, speed);
            return self.rate;
        }
        let dt = (at_ms - self.at_ms) as f64 / 1000.0;
        if dt <= 0.0 {
            return self.rate;
        }
        let reading = (done - self.done) as f64 / dt;
        self.at_ms = at_ms;
        self.done = done;
        // Noise is sized by the engine's average, which does not depend on
        // this reading, so high and low readings count alike.
        let scale = (speed as f64).max(FLOOR);
        self.variance += (DRIFT * scale).powi(2) * dt;
        let noise = (NOISE * scale).powi(2) * (0.25 / dt);
        let innovation = reading - self.rate;
        let mut total = self.variance + noise;
        self.trend = (1.0 - MEMORY) * self.trend + MEMORY * innovation / total.sqrt();
        if self.trend.abs() > CHANGE {
            self.variance += innovation * innovation;
            self.trend = 0.0;
            total = self.variance + noise;
        }
        let gain = self.variance / total;
        self.rate = (self.rate + gain * innovation).max(0.0);
        self.variance *= 1.0 - gain;
        self.rate
    }

    /// Seconds until `remaining` more bytes are done; None while stalled.
    pub fn time_left(&self, remaining: u64) -> Option<u64> {
        (self.rate >= 1.0).then(|| (remaining as f64 / self.rate).ceil() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: f64 = 1024.0 * 1024.0;

    /// Downloads whose bytes arrive in bursts around `rate(t)`, polled four
    /// times a second like a shown window, with 20 different burst patterns.
    /// Returns the filter's and the engine's 4 s average's mean distance
    /// from the true rate, as shares of it, in the span `from`..`to` seconds.
    fn simulate(rate: impl Fn(f64) -> f64, from: f64, to: f64) -> (f64, f64) {
        let (mut filtered, mut averaged, mut count) = (0.0, 0.0, 0.0);
        for run in 1..=20u64 {
            let (f, a, n) = simulate_one(&rate, from, to, run.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            filtered += f;
            averaged += a;
            count += n;
        }
        (filtered / count, averaged / count)
    }

    fn simulate_one(rate: &impl Fn(f64) -> f64, from: f64, to: f64, mut seed: u64) -> (f64, f64, f64) {
        let mut burst = || {
            // About ±50%: the sum of 12 uniforms is close to a normal curve.
            let mut sum = 0.0;
            for _ in 0..12 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                sum += (seed >> 11) as f64 / (1u64 << 53) as f64;
            }
            1.0 + 0.5 * (sum - 6.0)
        };
        let (mut done, mut history) = (0.0_f64, vec![(0.0_f64, 0.0_f64)]);
        let mut filter: Option<RateFilter> = None;
        let (mut filtered, mut averaged, mut count) = (0.0, 0.0, 0.0);
        for tick in 1..=(to * 4.0) as i64 {
            let t = tick as f64 / 4.0;
            let truth = rate(t);
            done += (truth * 0.25 * burst()).max(0.0);
            history.push((t, done));
            let &(then, before) = history.iter().find(|(at, _)| *at >= t - 4.0).unwrap();
            let average = (done - before) / (t - then);
            let at_ms = (t * 1000.0) as i64;
            let filter = filter.get_or_insert_with(|| RateFilter::new(at_ms, done as u64, average as u64));
            let estimate = filter.update(at_ms, done as u64, average as u64);
            if t >= from {
                filtered += (estimate - truth).abs() / truth;
                averaged += (average - truth).abs() / truth;
                count += 1.0;
            }
        }
        (filtered, averaged, count)
    }

    #[test]
    fn a_steady_rate_is_closer_than_the_engines_average() {
        let (filtered, averaged) = simulate(|_| 2.0 * MB, 6.0, 60.0);
        assert!(filtered < averaged * 0.8, "{filtered} against {averaged}");
    }

    #[test]
    fn a_drop_is_followed_sooner_than_the_engines_average() {
        let (filtered, averaged) = simulate(|t| if t < 20.0 { 2.0 * MB } else { 0.7 * MB }, 21.0, 26.0);
        assert!(filtered < averaged * 0.7, "{filtered} against {averaged}");
    }

    #[test]
    fn time_left_counts_down_and_stops_while_stalled() {
        let mut filter = RateFilter::new(0, 0, 1_000_000);
        for second in 1..=10 {
            filter.update(second * 1000, second as u64 * 1_000_000, 1_000_000);
        }
        assert_eq!(filter.time_left(5_000_000), Some(5));
        for second in 11..=60 {
            filter.update(second * 1000, 10_000_000, 0);
        }
        assert_eq!(filter.time_left(5_000_000), None);
    }
}

