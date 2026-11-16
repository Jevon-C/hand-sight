//! The result key: a fingerprint of everything that determines a result.
//!
//! Two runs with the same video bytes, test version, hand model, metrics and settings have the
//! same key, so a second run finds the first result instead of repeating the work, and a change
//! to any of them gives a different key instead of overwriting an earlier result.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::media::hex;

/// Everything that determines a result.
#[derive(Debug, Clone, Copy)]
pub struct KeyInputs<'a> {
    /// Id of the video (the hash of its bytes).
    pub video_id: &'a str,
    /// Id of the test.
    pub test_id: &'a str,
    /// Version of the test plug-in.
    pub test_version: &'a str,
    /// Fingerprint of the hand model.
    pub model_hash: &'a str,
    /// Every setting of the test, defaults included.
    pub parameters: &'a BTreeMap<String, f64>,
    /// The metrics asked for, in any order.
    pub metrics: &'a [String],
}

/// Compute the key, as 64 lowercase hex digits.
///
/// The order of the metrics does not matter. Settings are compared by their exact bit pattern
/// (with `-0.0` treated as `0.0`), so no rounding can make two different settings collide.
#[must_use]
pub fn result_key(inputs: &KeyInputs<'_>) -> String {
    let mut metrics: Vec<&str> = inputs.metrics.iter().map(String::as_str).collect();
    metrics.sort_unstable();
    metrics.dedup();
    let mut text = String::from("axonhand-result-key-v1\n");
    for (name, value) in [
        ("video", inputs.video_id),
        ("test", inputs.test_id),
        ("version", inputs.test_version),
        ("model", inputs.model_hash),
    ] {
        // Lengths are written so that no value can run into the next field.
        text.push_str(&format!("{name}:{}:{value}\n", value.len()));
    }
    text.push_str(&format!("metrics:{}\n", metrics.len()));
    for metric in metrics {
        text.push_str(&format!("m:{}:{metric}\n", metric.len()));
    }
    text.push_str(&format!("parameters:{}\n", inputs.parameters.len()));
    for (name, value) in inputs.parameters {
        let value = if *value == 0.0 { 0.0 } else { *value };
        text.push_str(&format!(
            "p:{}:{name}={:016x}\n",
            name.len(),
            value.to_bits()
        ));
    }
    hex(&Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[derive(Clone)]
    struct Case {
        video: &'static str,
        test: &'static str,
        version: &'static str,
        model: &'static str,
        parameters: BTreeMap<String, f64>,
        metrics: Vec<String>,
    }

    impl Case {
        fn base() -> Self {
            Self {
                video: "video",
                test: "test",
                version: "1.0.0",
                model: "model",
                parameters: BTreeMap::from([("a".to_owned(), 0.5), ("b".to_owned(), 3.0)]),
                metrics: vec!["x".to_owned(), "y".to_owned()],
            }
        }

        fn key(&self) -> String {
            result_key(&KeyInputs {
                video_id: self.video,
                test_id: self.test,
                test_version: self.version,
                model_hash: self.model,
                parameters: &self.parameters,
                metrics: &self.metrics,
            })
        }
    }

    #[test]
    fn the_key_is_stable_and_64_hex_digits() {
        let key = Case::base().key();
        assert_eq!(key, Case::base().key());
        assert!(crate::media::is_video_id(&key));
    }

    #[test]
    fn changing_any_input_changes_the_key() {
        let base = Case::base().key();
        type Edit = Box<dyn Fn(&mut Case)>;
        let edits: Vec<Edit> = vec![
            Box::new(|c| c.video = "video2"),
            Box::new(|c| c.test = "test2"),
            Box::new(|c| c.version = "1.0.1"),
            Box::new(|c| c.model = "model2"),
            Box::new(|c| {
                c.parameters.insert("a".to_owned(), 0.5000001);
            }),
            Box::new(|c| c.metrics = vec!["x".to_owned()]),
        ];
        let keys: Vec<String> = edits
            .iter()
            .map(|edit| {
                let mut case = Case::base();
                edit(&mut case);
                case.key()
            })
            .collect();
        assert!(keys.iter().all(|k| *k != base));
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }

    #[test]
    fn metric_order_and_repeats_do_not_matter() {
        let mut case = Case::base();
        case.metrics = vec!["y".to_owned(), "x".to_owned(), "y".to_owned()];
        assert_eq!(Case::base().key(), case.key());
    }

    #[test]
    fn negative_zero_equals_zero() {
        let mut plus = Case::base();
        plus.parameters = BTreeMap::from([("a".to_owned(), 0.0)]);
        let mut minus = plus.clone();
        minus.parameters = BTreeMap::from([("a".to_owned(), -0.0)]);
        assert_eq!(plus.key(), minus.key());
    }

    #[test]
    fn fields_cannot_run_into_each_other() {
        // "ab" + "c" must not collide with "a" + "bc".
        let mut a = Case::base();
        a.video = "ab";
        a.test = "c";
        let mut b = Case::base();
        b.video = "a";
        b.test = "bc";
        assert_ne!(a.key(), b.key());
    }

    proptest! {
        #[test]
        fn equal_inputs_give_equal_keys_and_any_value_change_is_seen(
            value in -1.0e6f64..1.0e6, other in -1.0e6f64..1.0e6,
        ) {
            let p1 = BTreeMap::from([("a".to_owned(), value)]);
            let p2 = BTreeMap::from([("a".to_owned(), other)]);
            let metrics = vec!["x".to_owned()];
            let make = |p: &BTreeMap<String, f64>| result_key(&KeyInputs {
                video_id: "v", test_id: "t", test_version: "1.0.0", model_hash: "m",
                parameters: p, metrics: &metrics,
            });
            prop_assert_eq!(make(&p1), make(&p1.clone()));
            if value.to_bits() != other.to_bits() && !(value == 0.0 && other == 0.0) {
                prop_assert_ne!(make(&p1), make(&p2));
            }
        }

        #[test]
        fn any_permutation_of_metrics_gives_the_same_key(
            metrics in proptest::collection::vec("[a-z]{1,6}", 1..6),
            seed in 0usize..100,
        ) {
            let params = BTreeMap::new();
            let mut shuffled = metrics.clone();
            shuffled.rotate_left(seed % metrics.len());
            let make = |m: &[String]| result_key(&KeyInputs {
                video_id: "v", test_id: "t", test_version: "1.0.0", model_hash: "m",
                parameters: &params, metrics: m,
            });
            prop_assert_eq!(make(&metrics), make(&shuffled));
        }
    }
}
