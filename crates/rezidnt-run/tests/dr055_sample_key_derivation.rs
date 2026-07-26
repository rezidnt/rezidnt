//! DR-055 ORACLE — sample-key derivation PROPERTIES (DR-055 §Decision 1;
//! trials slice B). The daemon derives each sample's spawn key as a
//! deterministic function of the trial's ONE idempotency key plus its
//! (variant, sample-index) pair, and the derived key resolves through the
//! EXISTING per-workspace `spawn_keys` map — the dedup RULE (same key => same
//! run, scope per workspace) is untouched; only the key SPACE is extended, by
//! the caller of the map. That is DR-048's binding idempotency clause
//! discharged in plain words, and it is the load-bearing behavioral claim this
//! board judges:
//!
//! - re-running the same trial re-derives IDENTICAL keys, so a retry dedupes
//!   and spawns nothing new;
//! - N samples of one trial get N DISTINCT keys, so N samples are N runs;
//! - keys are collision-resistant ACROSS trials, so two experiments can never
//!   silently share (and therefore swallow) each other's samples.
//!
//! ## What is deliberately NOT pinned
//!
//! DR-055 §Decision 1 leaves the exact derivation (hash choice, input
//! framing/encoding) to the implementer, "flagged not silent". NO test here
//! asserts a specific hash, prefix, length, or encoding — pinning one would be
//! the oracle overreaching into a decision the record left open. Only the
//! ruled PROPERTIES are judged.
//!
//! ## API surface this board PINS (the one narrowing, disclosed)
//!
//! ```ignore
//! // crates/rezidnt-run/src/trial.rs (new module; house home of the spawn
//! // machinery the daemon's open_trial handler drives)
//! pub fn derive_sample_key(trial_key: &str, variant_index: usize, sample_index: u64) -> String
//! ```
//!
//! The record says "(variant, sample-index)"; this board pins the variant leg
//! as the variant's INDEX into the fact's verbatim-ordered `variants` list,
//! not the (agent, harness, model?) triple. Disclosed reasoning, so a
//! disagreeing implementer argues with the reason and not a guess: (1) the
//! ontology makes variant ORDER semantic ("VERBATIM on the fact, ordered as
//! requested"), so the index is well-defined and log-derivable; (2)
//! triple-framing collides silently when a matrix legally names the same
//! triple twice — V x N requested, fewer distinct keys derived, samples
//! swallowed by dedup with no refusal anywhere — which is exactly the
//! silent-wrong class this project keeps failing on. The function must live in
//! a LIBRARY crate (not `bins/rezidentd`) so this judge and the daemon share
//! one derivation — two copies that drift would break log-derivability.
//!
//! ## RED MODE (against the tree at cut time — session 33, post-`bcd0db9`)
//!
//! COMPILE-RED: `rezidnt_run::trial` does not exist (no `trial` module, no
//! `derive_sample_key` symbol anywhere in `crates/rezidnt-run/src/` — verified
//! by grep this session). Red for the right reason: the derivation this board
//! judges is unbuilt.

use rezidnt_run::trial::derive_sample_key;

/// The ontology's constraints on `agent.spawned.idempotency_key` v1, which
/// every derived key must satisfy because that is the field it lands on:
/// non-empty, at most 256 bytes UTF-8.
const SPAWN_KEY_MAX_BYTES: usize = 256;

const TRIAL_KEY: &str = "dr055-trial-key-alpha";
const OTHER_TRIAL_KEY: &str = "dr055-trial-key-beta";

/// Determinism, the retry half of Decision 1: the same (trial key, variant,
/// sample) re-derives the SAME key on every call — a re-run of the whole trial
/// re-derives the identical key set, each sample hits the existing
/// `spawn_keys` map, and NOTHING new spawns. No clocks, no randomness, no
/// per-process salt.
#[test]
fn rerunning_the_same_trial_rederives_identical_keys() {
    for variant in 0..3usize {
        for sample in 0..3u64 {
            let first = derive_sample_key(TRIAL_KEY, variant, sample);
            let second = derive_sample_key(TRIAL_KEY, variant, sample);
            assert_eq!(
                first, second,
                "derivation must be a pure function of (trial_key, variant, \
                 sample) — a nondeterministic key breaks retry-dedup and \
                 double-spawns the matrix (DR-055 §Decision 1)"
            );
        }
    }
}

/// The other half: N samples of ONE variant get N DISTINCT keys. This is the
/// exact fatality DR-055 §Context 1 opens with — under today's per-task keys,
/// N samples of one task are one run; the derivation exists to make them N.
#[test]
fn n_samples_of_one_variant_get_n_distinct_keys() {
    let keys: Vec<String> = (0..32u64)
        .map(|sample| derive_sample_key(TRIAL_KEY, 0, sample))
        .collect();
    let distinct: std::collections::BTreeSet<&String> = keys.iter().collect();
    assert_eq!(
        distinct.len(),
        keys.len(),
        "same variant, different sample-index => different key; a collision \
         here silently dedupes two requested samples into one run"
    );
}

/// The whole matrix: V x N requested => V x N distinct keys. Distinctness must
/// hold ACROSS the variant axis too — a derivation that ignored the variant
/// leg would collapse every variant's sample s into one run.
#[test]
fn a_whole_matrix_derives_v_times_n_distinct_keys() {
    let (v, n) = (8usize, 8u64);
    let mut keys = std::collections::BTreeSet::new();
    for variant in 0..v {
        for sample in 0..n {
            keys.insert(derive_sample_key(TRIAL_KEY, variant, sample));
        }
    }
    assert_eq!(
        keys.len(),
        v * n as usize,
        "an {v}x{n} matrix must derive exactly {v}x{n} distinct sample keys — \
         every collision is a requested sample that silently never spawns"
    );
}

/// Collision resistance ACROSS trials: two different trial keys must never
/// derive a common sample key, or one experiment's retry would silently adopt
/// (and suppress) another experiment's runs through the shared per-workspace
/// `spawn_keys` map.
#[test]
fn two_trials_never_share_a_sample_key() {
    let mut a = std::collections::BTreeSet::new();
    let mut b = std::collections::BTreeSet::new();
    for variant in 0..4usize {
        for sample in 0..4u64 {
            a.insert(derive_sample_key(TRIAL_KEY, variant, sample));
            b.insert(derive_sample_key(OTHER_TRIAL_KEY, variant, sample));
        }
    }
    assert!(
        a.is_disjoint(&b),
        "distinct trial keys must derive disjoint sample-key sets — a shared \
         key crosses two experiments' dedup scopes (DR-055 §Decision 1 \
         'collision-resistant across trials')"
    );
}

/// The derived key lands on `agent.spawned.idempotency_key`, so it must
/// satisfy that field's ratified v1 constraints — non-empty, <= 256 bytes
/// UTF-8 — for EVERY input the tool admits, including a trial key already at
/// the 256-byte cap. A 300-byte derived key would be refused by the spawn
/// path's own key guard and every sample would die at the door.
#[test]
fn derived_keys_satisfy_the_spawn_key_constraints() {
    let at_cap = "k".repeat(SPAWN_KEY_MAX_BYTES);
    for trial_key in [TRIAL_KEY, at_cap.as_str()] {
        for variant in [0usize, 7, 63] {
            for sample in [0u64, 7, 9999] {
                let key = derive_sample_key(trial_key, variant, sample);
                assert!(!key.is_empty(), "a derived key is never empty");
                assert!(
                    key.len() <= SPAWN_KEY_MAX_BYTES,
                    "a derived key must fit agent.spawned.idempotency_key's \
                     ratified <= {SPAWN_KEY_MAX_BYTES}-byte bound even when the \
                     trial key is at the cap (got {} bytes)",
                    key.len()
                );
            }
        }
    }
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Determinism + cross-trial collision resistance over ARBITRARY trial
        /// keys (any non-empty printable key up to the 256-byte cap): equal
        /// inputs derive equal keys; unequal trial keys never derive a common
        /// key over a 4x4 matrix. Probabilistic inputs, exact properties —
        /// nothing here constrains the hash itself.
        #[test]
        fn determinism_and_cross_trial_disjointness(
            key_a in "[a-zA-Z0-9_.:-]{1,64}",
            key_b in "[a-zA-Z0-9_.:-]{1,64}",
        ) {
            for variant in 0..4usize {
                for sample in 0..4u64 {
                    prop_assert_eq!(
                        derive_sample_key(&key_a, variant, sample),
                        derive_sample_key(&key_a, variant, sample),
                        "pure function: same inputs, same key"
                    );
                }
            }
            prop_assume!(key_a != key_b);
            let a: std::collections::BTreeSet<String> = (0..4usize)
                .flat_map(|v| (0..4u64).map(move |s| (v, s)))
                .map(|(v, s)| derive_sample_key(&key_a, v, s))
                .collect();
            let b: std::collections::BTreeSet<String> = (0..4usize)
                .flat_map(|v| (0..4u64).map(move |s| (v, s)))
                .map(|(v, s)| derive_sample_key(&key_b, v, s))
                .collect();
            prop_assert!(
                a.is_disjoint(&b),
                "distinct trial keys {:?} / {:?} derived overlapping sample keys",
                key_a, key_b
            );
        }

        /// The (variant, sample) leg is injective for ONE trial key drawn
        /// arbitrarily: a 6x6 matrix always yields 36 distinct keys. Guards
        /// against framings that concatenate ambiguously (e.g. "1"+"11" vs
        /// "11"+"1") without pinning any particular framing.
        #[test]
        fn matrix_cells_are_injective_for_any_trial_key(
            key in "[a-zA-Z0-9_.:-]{1,64}",
        ) {
            let keys: std::collections::BTreeSet<String> = (0..6usize)
                .flat_map(|v| (0..6u64).map(move |s| (v, s)))
                .map(|(v, s)| derive_sample_key(&key, v, s))
                .collect();
            prop_assert_eq!(keys.len(), 36, "6x6 => 36 distinct keys, for every trial key");
        }
    }
}
