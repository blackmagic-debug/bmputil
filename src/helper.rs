use std::thread;
use std::time::{Duration, Instant};

pub fn retry_with_delay_time<T, E, F>(f: F, max_duration: Duration, delay: Duration) -> Result<T, E>
where
    F: Fn(f64) -> Result<T, E>
{
    retry_with_delay_time_with_match(f, |m| m.is_ok(), max_duration, delay)
}

pub fn retry_with_delay_time_with_match<T, E, F, M>(f: F, matcher: M, max_duration: Duration, delay: Duration) -> Result<T, E>
where
    F: Fn(f64) -> Result<T, E>,
    M: Fn(&Result<T, E>) -> bool,
{
    let start = Instant::now();
    let mut last_res = None;

    while start.elapsed() < max_duration {
        let fraction = start.elapsed().as_secs_f64() / max_duration.as_secs_f64();
        let result = f(fraction);

        if matcher(&result) {
            return result;
        }

        last_res = Some(result);
        thread::sleep(delay);
    }

    last_res.expect("No attempts made")
}
