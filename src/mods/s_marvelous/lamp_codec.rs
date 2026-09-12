//! Pure wire codec for the S-MFC lamp set (server-upload design §5.2):
//! `mcode:chart:clearkind|…`, decimal, `chart` 0..=4 single / 5..=9 double.
//! Shares its test vectors with the bemani-buddy side. Std-only so
//! `scripts/validate_s_marvelous.sh` mounts it beside `upload.rs`.

use std::collections::HashSet;

use super::upload::CLEAR_KIND_SMFC;

/// Wire name of the load-side field (bemani-buddy `option.smarv_scores`).
pub const WIRE_NAME: &str = "smarv_scores";

/// One decoded entry of the wire list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entry {
    pub mcode: i32,
    pub chart: i32,
    pub clearkind: i32,
}

/// Decode `mcode:chart:clearkind|…` leniently: malformed entries are skipped
/// individually; an empty string is the empty list. Accepts any clear kind
/// (the consumer filters).
pub fn decode(text: &str) -> Vec<Entry> {
    text.split('|')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let mut parts = item.split(':');
            let mcode = parts.next()?.trim().parse::<i32>().ok()?;
            let chart = parts.next()?.trim().parse::<i32>().ok()?;
            let clearkind = parts.next()?.trim().parse::<i32>().ok()?;
            if parts.next().is_some() || mcode < 0 || !(0..=9).contains(&chart) {
                return None;
            }
            Some(Entry {
                mcode,
                chart,
                clearkind,
            })
        })
        .collect()
}

/// Encode entries in wire form, sorted by `(mcode, chart)` (the backend's
/// canonical order; used by the shared test vectors).
pub fn encode(entries: &[Entry]) -> String {
    let mut v: Vec<Entry> = entries.to_vec();
    v.sort_by_key(|e| (e.mcode, e.chart));
    v.iter()
        .map(|e| format!("{}:{}:{}", e.mcode, e.chart, e.clearkind))
        .collect::<Vec<_>>()
        .join("|")
}

/// Pure core of [`on_load`]: the S-MFC set a wire list denotes.
pub fn smfc_set(entries: &[Entry]) -> HashSet<(i32, i32)> {
    entries
        .iter()
        .filter(|e| e.clearkind == CLEAR_KIND_SMFC)
        .map(|e| (e.mcode, e.chart))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_encode_round_trip_sorted() {
        let text = "38548:3:11|1234:8:11|38548:0:11";
        let entries = decode(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(encode(&entries), "1234:8:11|38548:0:11|38548:3:11");
        assert_eq!(decode(&encode(&entries)), {
            let mut v = entries.clone();
            v.sort_by_key(|e| (e.mcode, e.chart));
            v
        });
    }

    #[test]
    fn decode_skips_malformed_entries_and_accepts_empty() {
        assert!(decode("").is_empty());
        assert!(decode("|||").is_empty());
        let entries = decode("abc|1:2|100:3:11|200:12:11|-5:1:11|300:4:11:9|400:1:10");
        assert_eq!(
            entries,
            vec![
                Entry {
                    mcode: 100,
                    chart: 3,
                    clearkind: 11
                },
                Entry {
                    mcode: 400,
                    chart: 1,
                    clearkind: 10
                },
            ]
        );
    }

    #[test]
    fn smfc_set_filters_on_clear_kind_11() {
        let set = smfc_set(&decode("1:0:11|2:5:10|3:9:11"));
        assert_eq!(set, HashSet::from([(1, 0), (3, 9)]));
    }
}
