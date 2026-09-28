//! Java zone ids (`java.time.ZoneId.of`) as the scheduler evaluates them:
//! which ids are valid, and the fixed offset a non-region id stands for.
//! A scheduled job's `timezone` and a function's schedule both use them.

use std::str::FromStr;

/// Java `ZoneId.of`: `Z`, an offset (`+h`, `+hh`, `+hh:mm`, `+hhmm`,
/// `+hh:mm:ss`, `+hhmmss`, at most ±18:00), `UTC`/`GMT`/`UT` alone or
/// followed by an offset, or a tz database region id the scheduler can
/// evaluate ([`region_valid`]).
pub fn zone_id_valid(id: &str) -> bool {
    if id == "Z" || id.starts_with(['+', '-']) {
        return offset_valid(id);
    }
    for prefix in ["UTC", "GMT", "UT"] {
        if id == prefix {
            return true;
        }
        if let Some(rest) = id.strip_prefix(prefix) {
            if rest.starts_with(['+', '-']) {
                return offset_valid(rest);
            }
        }
    }
    region_valid(id)
}

/// The fixed offset, in seconds east of UTC, that a Java zone id which is
/// not a region stands for: `Z`, an offset, or `UTC`/`GMT`/`UT` alone or
/// followed by an offset. `None` for a region id (or an invalid one).
pub fn java_fixed_offset_seconds(id: &str) -> Option<i32> {
    if !zone_id_valid(id) {
        return None;
    }
    let offset = if id == "Z" || id.starts_with(['+', '-']) {
        id
    } else {
        let rest = ["UTC", "GMT", "UT"]
            .iter()
            .find_map(|prefix| id.strip_prefix(prefix))?;
        if rest.is_empty() {
            return Some(0);
        }
        rest
    };
    if offset == "Z" {
        return Some(0);
    }
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let (h, m, s) = offset_parts(&offset[1..])?;
    Some(sign * (h * 3600 + m * 60 + s) as i32)
}

/// Java `ZoneOffset.of`.
fn offset_valid(id: &str) -> bool {
    if id == "Z" {
        return true;
    }
    match id.strip_prefix(['+', '-']).and_then(offset_parts) {
        Some((h, m, s)) => h <= 18 && m <= 59 && s <= 59 && (h < 18 || (m == 0 && s == 0)),
        None => false,
    }
}

/// The hours, minutes and seconds of an offset after its sign.
fn offset_parts(body: &str) -> Option<(u32, u32, u32)> {
    if !body.is_ascii() {
        return None;
    }
    let num = |s: &str| -> Option<u32> {
        if s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    match body.len() {
        1 if body.bytes().all(|b| b.is_ascii_digit()) => Some((body.parse().ok()?, 0, 0)),
        2 => Some((num(body)?, 0, 0)),
        4 => Some((num(&body[0..2])?, num(&body[2..4])?, 0)),
        5 if &body[2..3] == ":" => Some((num(&body[0..2])?, num(&body[3..5])?, 0)),
        6 => Some((num(&body[0..2])?, num(&body[2..4])?, num(&body[4..6])?)),
        8 if &body[2..3] == ":" && &body[5..6] == ":" => {
            Some((num(&body[0..2])?, num(&body[3..5])?, num(&body[6..8])?))
        }
        _ => None,
    }
}

/// Region ids in chrono-tz's tz database that the JDK's leaves out: the
/// three that clash with its legacy short ids, and `ROC`.
const NOT_IN_JDK: [&str; 4] = ["EST", "HST", "MST", "ROC"];

/// Java `ZoneRegion.ofId(id, true)`: the id's shape, then a tz database
/// lookup, in chrono-tz's database without the ids the JDK's (25) leaves
/// out. The JDK also keeps the legacy `SystemV/*` ids, which chrono-tz's
/// database lacks: the scheduler could never evaluate them, so they are
/// refused (owner decision 2 of 2026-09-25) rather than accepted and never
/// fired.
fn region_valid(id: &str) -> bool {
    let mut chars = id.chars();
    let shaped = id.len() >= 2
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "~/._+-".contains(c));
    shaped && chrono_tz::Tz::from_str(id).is_ok() && !NOT_IN_JDK.contains(&id)
}
