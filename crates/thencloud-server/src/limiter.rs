//! In-memory fixed-window limiter for failed authentication attempts.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::util::now;

pub struct Limiter {
    failures: Mutex<HashMap<String, (u32, i64)>>,
    max: u32,
    window: i64,
}

impl Limiter {
    pub fn new(max: u32, window_secs: i64) -> Self {
        Limiter {
            failures: Mutex::new(HashMap::new()),
            max,
            window: window_secs,
        }
    }

    /// Failures allowed in a window by default.
    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn blocked(&self, key: &str) -> bool {
        self.blocked_at(key, self.max)
    }

    /// Blocked after `max` failures in the window, instead of the default.
    pub fn blocked_at(&self, key: &str, max: u32) -> bool {
        let map = self.failures.lock().unwrap();
        map.get(key)
            .is_some_and(|&(n, start)| now() - start < self.window && n >= max)
    }

    pub fn fail(&self, key: &str) {
        let mut map = self.failures.lock().unwrap();
        let t = now();
        let e = map.entry(key.to_string()).or_insert((0, t));
        if t - e.1 >= self.window {
            *e = (0, t);
        }
        e.0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.failures.lock().unwrap().remove(key);
    }

    pub fn prune(&self) {
        let t = now();
        self.failures
            .lock()
            .unwrap()
            .retain(|_, &mut (_, start)| t - start < self.window);
    }
}
