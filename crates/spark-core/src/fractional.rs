//! Fractional-index sort keys over printable ASCII (33..=126).
//!
//! A key is a base-94 fraction in (0, 1). `key_between(a, b)` returns a key strictly between
//! `a` and `b`, so reordering or reparenting a node rewrites one string. Keys never end in the
//! lowest digit, which keeps room in front of every key.

const MIN: u8 = 33;
const BASE: u16 = 94;

fn digit(k: &[u8], i: usize) -> u16 {
    k.get(i).map_or(0, |b| (*b - MIN) as u16)
}

fn midpoint(a: &[u8], b: Option<&[u8]>, step: bool, out: &mut Vec<u8>) {
    if let Some(b) = b {
        let mut n = 0;
        while n < b.len() && digit(a, n) == digit(b, n) {
            n += 1;
        }
        if n > 0 {
            out.extend_from_slice(&b[..n]);
            return midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..]), false, out);
        }
    }
    let da = digit(a, 0);
    let Some(b) = b else {
        // Open upper end: appends step by one digit so repeated appends grow keys slowly.
        if da == BASE - 1 {
            out.push(a[0]);
            midpoint(&a[1..], None, step, out);
        } else if a.is_empty() && step && !out.is_empty() {
            // Continuing past a run of max digits: use the whole next digit's range.
            out.push(1 + MIN);
        } else if a.is_empty() || !step {
            out.push(((da + BASE) / 2) as u8 + MIN);
        } else {
            out.push(da as u8 + 1 + MIN);
        }
        return;
    };
    let db = digit(b, 0);
    if db - da > 1 {
        out.push(((da + db) / 2) as u8 + MIN);
    } else if b.len() > 1 {
        out.push(b[0]);
    } else {
        out.push(da as u8 + MIN);
        midpoint(a.get(1..).unwrap_or(&[]), None, false, out);
    }
}

pub fn is_valid_key(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| (MIN..=126).contains(&b)) && !k.ends_with(MIN as char)
}

/// A key strictly between `a` and `b` (either may be open). Panics if `a >= b`.
pub fn key_between(a: Option<&str>, b: Option<&str>) -> String {
    if let (Some(a), Some(b)) = (a, b) {
        assert!(a < b, "key_between: {a:?} >= {b:?}");
    }
    let mut out = Vec::new();
    midpoint(a.unwrap_or("").as_bytes(), b.map(str::as_bytes), b.is_none(), &mut out);
    String::from_utf8(out).unwrap()
}

/// `n` increasing keys between `a` and `b`, spread by bisection so bulk inserts stay short.
pub fn keys_between(a: Option<&str>, b: Option<&str>, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    // Bisect toward a concrete upper bound; an open end would only step one digit at a time.
    let hi;
    let b = match b {
        Some(b) => Some(b),
        None => {
            let tildes = a.map_or(0, |a| a.bytes().take_while(|c| *c == b'~').count());
            hi = "~".repeat(tildes + 1);
            Some(hi.as_str())
        }
    };
    let mid = key_between(a, b);
    let left = (n - 1) / 2;
    let mut keys = keys_between(a, Some(&mid), left);
    let right = keys_between(Some(&mid), b, n - 1 - left);
    keys.push(mid);
    keys.extend(right);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn between_is_strictly_ordered() {
        let mid = key_between(None, None);
        let before = key_between(None, Some(&mid));
        let after = key_between(Some(&mid), None);
        assert!(before < mid && mid < after);
        assert!([&mid, &before, &after].iter().all(|k| is_valid_key(k)));
    }

    #[test]
    fn repeated_inserts_stay_ordered_and_valid() {
        // Always insert right after `lo`: the worst case for key growth.
        let lo = key_between(None, None);
        let mut hi = key_between(Some(&lo), None);
        for _ in 0..500 {
            let k = key_between(Some(&lo), Some(&hi));
            assert!(lo < k && k < hi, "{lo} < {k} < {hi}");
            assert!(is_valid_key(&k));
            hi = k;
        }
        // And always prepend at the front.
        let mut first = key_between(None, None);
        for _ in 0..500 {
            let k = key_between(None, Some(&first));
            assert!(k < first && is_valid_key(&k));
            first = k;
        }
    }

    #[test]
    fn bulk_and_appended_keys_stay_short() {
        let keys = keys_between(None, None, 20_000);
        assert_eq!(keys.len(), 20_000);
        if let Some(w) = keys.windows(2).find(|w| w[0] >= w[1]) {
            panic!("{:?}", w);
        }
        let max = keys.iter().map(String::len).max().unwrap();
        assert!(max <= 4 && keys.iter().all(|k| is_valid_key(k)), "max key length {max}");
        let mut last = key_between(None, None);
        for _ in 0..1000 {
            let k = key_between(Some(&last), None);
            assert!(k > last && is_valid_key(&k));
            last = k;
        }
        assert!(last.len() <= 12);
    }

    #[test]
    fn adjacent_digits_take_the_longer_side() {
        let k = key_between(Some("a"), Some("b"));
        assert!("a" < k.as_str() && k.as_str() < "b");
        let k = key_between(Some("a"), Some("bV"));
        assert!("a" < k.as_str() && k.as_str() < "bV");
    }
}
