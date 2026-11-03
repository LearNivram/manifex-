//! Kubernetes resource quantity parsing (CPU / memory).

/// Parses a CPU quantity into millicores ("250m" -> 250, "2" -> 2000, "1500000n" -> 1.5).
pub fn cpu_millis(q: &str) -> Option<f64> {
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    let (num, factor) = if let Some(n) = q.strip_suffix('n') {
        (n, 1e-6)
    } else if let Some(n) = q.strip_suffix('u') {
        (n, 1e-3)
    } else if let Some(n) = q.strip_suffix('m') {
        (n, 1.0)
    } else {
        (q, 1000.0)
    };
    num.parse::<f64>().ok().map(|v| v * factor)
}

/// Parses a memory / storage quantity into bytes.
pub fn bytes(q: &str) -> Option<f64> {
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    const SUFFIXES: &[(&str, f64)] = &[
        ("Ki", 1024.0),
        ("Mi", 1024.0 * 1024.0),
        ("Gi", 1024.0 * 1024.0 * 1024.0),
        ("Ti", 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Pi", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Ei", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("k", 1e3),
        ("K", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
        ("m", 1e-3),
    ];
    for (suffix, factor) in SUFFIXES {
        if let Some(n) = q.strip_suffix(suffix) {
            return n.parse::<f64>().ok().map(|v| v * factor);
        }
    }
    // plain number or scientific notation (e.g. "1e3")
    q.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu() {
        assert_eq!(cpu_millis("250m"), Some(250.0));
        assert_eq!(cpu_millis("2"), Some(2000.0));
        assert_eq!(cpu_millis("0.5"), Some(500.0));
        assert!((cpu_millis("1500000n").unwrap() - 1.5).abs() < 1e-9);
        assert_eq!(cpu_millis(""), None);
    }

    #[test]
    fn memory() {
        assert_eq!(bytes("1Ki"), Some(1024.0));
        assert_eq!(bytes("128Mi"), Some(134_217_728.0));
        assert_eq!(bytes("1G"), Some(1e9));
        assert_eq!(bytes("1000"), Some(1000.0));
        assert_eq!(bytes("1e3"), Some(1000.0));
    }
}
