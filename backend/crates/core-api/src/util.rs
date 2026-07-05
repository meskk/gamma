//! Small shared helpers.

/// Parse an env var as `T`, or fall back to `default` if it is unset. A PRESENT
/// but unparseable value is a misconfiguration and panics at startup — better
/// than silently running with a default the operator didn't intend.
pub fn env_parsed<T: std::str::FromStr>(name: &str, default: T) -> T {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .unwrap_or_else(|_| panic!("{name} is set but not a valid value: {v:?}")),
        Err(_) => default,
    }
}
