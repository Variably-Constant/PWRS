//! `#[pwrs::tiered]`: every tier this CPU supports gives the bits the
//! baseline gives. The tier is picked once per process, so the test runs
//! its own binary again once per `PWRS_CPU_MAX` tier of this architecture
//! and compares a digest of each run's outputs.

use std::process::Command;

#[pwrs::tiered]
fn squares(values: &[f32], out: &mut [f32]) {
    for (o, v) in out.iter_mut().zip(values) {
        *o = v * v + 0.5;
    }
}

#[pwrs::tiered]
fn ones(words: &[u64]) -> u64 {
    words.iter().map(|w| u64::from(w.count_ones())).sum()
}

#[pwrs::tiered]
fn total(values: &[f64]) -> f64 {
    values.iter().sum()
}

#[pwrs::tiered]
fn widest<T: Copy + PartialOrd, const N: usize>(rows: &[[T; N]]) -> Option<T> {
    rows.iter().flatten().copied().fold(None, |best, v| match best {
        Some(b) if b >= v => Some(b),
        _no_best_or_smaller => Some(v),
    })
}

#[pwrs::tiered]
fn matching(mut count: usize, haystack: &[u8], byte: u8) -> usize {
    for b in haystack {
        if *b == byte {
            count += 1;
        }
    }
    count
}

/// FNV-1a over raw bytes.
fn fold(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= u64::from(*b);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Every tiered fn over inputs a fixed generator makes, their output bits
/// folded into one value. The floats are raw bit patterns, so NaNs,
/// infinities and subnormals are among them.
fn digest() -> u64 {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let floats: Vec<f32> = (0..4099).map(|_| f32::from_bits((next() >> 32) as u32)).collect();
    let words: Vec<u64> = (0..1031).map(|_| next()).collect();
    let doubles: Vec<f64> = (0..2053).map(|_| (next() >> 11) as f64 / 9_007_199_254_740_992.0 - 0.5).collect();
    let rows: Vec<[u16; 7]> = (0..509).map(|_| std::array::from_fn(|_| next() as u16)).collect();
    let bytes: Vec<u8> = (0..8191).map(|_| (next() % 7) as u8).collect();

    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut out = vec![0.0f32; floats.len()];
    squares(&floats, &mut out);
    for v in &out {
        fold(&mut hash, &v.to_bits().to_le_bytes());
    }
    fold(&mut hash, &ones(&words).to_le_bytes());
    fold(&mut hash, &total(&doubles).to_bits().to_le_bytes());
    fold(&mut hash, &widest(&rows).expect("rows").to_le_bytes());
    let pairs: Vec<[f32; 2]> = floats.as_chunks::<2>().0.to_vec();
    fold(&mut hash, &widest(&pairs).expect("pairs").to_bits().to_le_bytes());
    fold(&mut hash, &matching(3, &bytes, 4).to_le_bytes());
    hash
}

#[test]
fn every_tier_this_cpu_reaches_gives_the_baseline_bits() {
    if std::env::var_os("PWRS_TIERED_CHILD").is_some() {
        let reached = if cfg!(target_arch = "aarch64") { pwrs::cpu::arm::tier().name() } else { pwrs::cpu::level().name() };
        println!("TIERED reached={reached} digest={:016x}", digest());
        return;
    }
    let caps: &[&str] = if cfg!(target_arch = "x86_64") {
        &["x86-64", "x86-64-v2", "x86-64-v3", "x86-64-v4", "native"]
    } else if cfg!(target_arch = "aarch64") {
        &["neon", "dotprod", "i8mm", "sve", "sve2", "native"]
    } else {
        &["native"]
    };
    let exe = std::env::current_exe().expect("the test binary's path");
    let mut runs: Vec<(&str, String, String)> = Vec::new();
    for cap in caps {
        let out = Command::new(&exe)
            .args(["every_tier_this_cpu_reaches_gives_the_baseline_bits", "--exact", "--nocapture", "--test-threads=1"])
            .env("PWRS_TIERED_CHILD", "1")
            .env("PWRS_CPU_MAX", cap)
            .output()
            .expect("the test binary runs again");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        // The harness prints the test's name on the same line first.
        let line = match text.lines().find_map(|l| l.find("TIERED reached=").map(|at| &l[at..])) {
            Some(line) => line.to_string(),
            None => panic!("PWRS_CPU_MAX={cap} printed no result:\n{text}\n{}", String::from_utf8_lossy(&out.stderr)),
        };
        let field = |name: &str| match line.split_whitespace().find_map(|w| w.strip_prefix(name)) {
            Some(value) => value.to_string(),
            None => panic!("no {name} in {line}"),
        };
        let (reached, digest) = (field("reached="), field("digest="));
        eprintln!("TIERED RUNNING PWRS_CPU_MAX={cap} reached={reached} digest={digest}");
        runs.push((cap, reached, digest));
    }
    let (_, base_tier, base) = &runs[0];
    for (cap, reached, digest) in &runs {
        assert_eq!(digest, base, "PWRS_CPU_MAX={cap} ran the {reached} copies, whose results differ from the {base_tier} copies'");
    }
    let mut reached: Vec<&str> = runs.iter().map(|r| r.1.as_str()).collect();
    reached.dedup();
    eprintln!("TIERED compared {} tiers: {}", reached.len(), reached.join(", "));
}
