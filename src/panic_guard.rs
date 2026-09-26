//! handle panics and convert them into `Result` errors

use std::panic::{self, UnwindSafe};

pub fn catch<T>(f: impl FnOnce() -> T + UnwindSafe) -> Result<T, String> {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(f);
    panic::set_hook(hook);
    result.map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::catch;

    #[test]
    fn passes_through_value() {
        assert_eq!(catch(|| 42), Ok(42));
    }

    #[test]
    fn panic_becomes_error() {
        assert_eq!(catch(|| -> () { panic!("boom") }), Err("boom".to_string()));
        assert_eq!(catch(|| -> () { panic!("{}", 1) }), Err("1".to_string()));
    }
}
