use anyhow::Result;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::Rng;
use rayon::prelude::*;
use statrs::statistics::Statistics;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lexicon_cid::Cid;
use rsky_pds::{
    common::{ipld::cid_for_cbor, struct_to_cbor},
    repo::mst::{util::leading_zeros_on_hash, MST},
    storage::SqlRepoReader,
};

#[derive(Debug)]
struct DatasetStats {
    size: usize,
    d_avg: f64,
    d_std: f64,
    d_max: f64,
    h_avg: f64,
    h_std: f64,
    h_max: f64,
}

// Helper struct for key generation
struct ControlledKeyGenerator {
    counter: Arc<AtomicU64>,
}

impl ControlledKeyGenerator {
    fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    fn generate_key(&self) -> Result<String> {
        let mut rng = rand::thread_rng();
        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let id = format!("{:016x}", rng.gen::<u64>() ^ counter);
        Ok(format!("com.example.record/{}", id))
    }
}

// Compute full distance metric: d(x,y) = sqrt((h_B(x) - h_B(y))^2 + (x - y)^2)
fn compute_d(key1: &str, key2: &str) -> Result<f64> {
    Ok(
        (comput_hash_zero_distance_squared(key1, key2)?
            + compute_edit_distance_square(key1, key2)?)
        .sqrt(),
    )
}

// Compute just the hash distance: sqrt((h_B(x) - h_B(y))^2)
fn compute_h(key1: &str, key2: &str) -> Result<f64> {
    Ok(comput_hash_zero_distance_squared(key1, key2)?.sqrt())
}

// Compute (x - y)^2 numerically
fn compute_edit_distance_square(key1: &str, key2: &str) -> Result<f64> {
    // Convert hex strings to big numbers
    let n1 = u128::from_str_radix(key1.split('/').nth(1).unwrap(), 16).unwrap() as i128;
    let n2 = u128::from_str_radix(key2.split('/').nth(1).unwrap(), 16).unwrap() as i128;
    Ok(((n1 - n2).abs() as f64).powi(2))
}

// Compute (h_B(x) - h_B(y))^2 hash zero distance squared
fn comput_hash_zero_distance_squared(key1: &str, key2: &str) -> Result<f64> {
    let h1 = leading_zeros_on_hash(&key1.as_bytes().to_vec())? as i64;
    let h2 = leading_zeros_on_hash(&key2.as_bytes().to_vec())? as i64;
    Ok((h1 - h2).pow(2) as f64)
}

fn analyze_dataset(entries: &[(String, Cid)]) -> Result<DatasetStats> {
    let mut d_distances = Vec::new();
    let mut h_distances = Vec::new();

    // Compute all pairwise distances
    for i in 0..entries.len() {
        for j in i + 1..entries.len() {
            let d = compute_d(&entries[i].0, &entries[j].0)?;
            let h = compute_h(&entries[i].0, &entries[j].0)?;
            // d_distances.push(d);
            h_distances.push(h);
        }
    }

    Ok(DatasetStats {
        size: entries.len(),
        d_avg: d_distances.clone().mean(),
        d_std: d_distances.clone().std_dev(),
        d_max: d_distances
            .into_iter()
            .fold(f64::NEG_INFINITY, |a, b| a.max(b)),
        h_avg: h_distances.clone().mean(),
        h_std: h_distances.clone().std_dev(),
        h_max: h_distances
            .into_iter()
            .fold(f64::NEG_INFINITY, |a, b| a.max(b)),
    })
}

fn generate_test_data(count: usize, storage: &mut SqlRepoReader) -> Result<Vec<(String, Cid)>> {
    let mut entries = Vec::with_capacity(count);
    let generator = ControlledKeyGenerator::new();

    for _ in 0..count {
        let key = generator.generate_key()?;
        let record = serde_json::json!({ "test": format!("{:016x}", rand::random::<u64>()) });
        let cid = cid_for_cbor(&record)?;
        let bytes = struct_to_cbor(record)?;
        storage.blocks.set(cid, bytes);
        entries.push((key, cid));
    }

    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(entries)
}

fn bench_add_records(c: &mut Criterion) {
    let mut group = c.benchmark_group("mst_operations");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(1500));
    for size in [100, 500, 1000].iter() {
        println!("");
        group.bench_with_input(BenchmarkId::new("add_records", size), size, |b, &size| {
            b.iter_custom(|iters| {
                let mut total_duration = std::time::Duration::ZERO;

                for iter in 0..iters {
                    let mut storage = SqlRepoReader::new(
                        None,
                        "did:example:123456789abcdefghi".to_string(),
                        None,
                    );
                    let mst = MST::create(storage.clone(), None, None).unwrap();
                    let data = generate_test_data(size, &mut storage).unwrap();

                    // Only analyze and report stats for first iteration to reduce noise
                    if iter == 0 {
                        let stats = analyze_dataset(&data).unwrap();
                        println!(
                            "{},{:.2},{:.2},{:.2}",
                            stats.size,
                            stats.h_avg,
                            stats.h_std,
                            stats.h_max
                        );
                    }

                    let start = Instant::now();
                    let mut mst = mst;
                    for (key, cid) in &data {
                        // in almost every instance i have seen we use None for known_zeros
                        // let zeros = leading_zeros_on_hash(&key.as_bytes().to_vec()).unwrap();
                        // mst = black_box(mst.add(key, *cid, Some(zeros)).unwrap());
                        mst = black_box(mst.add(key, *cid, None).unwrap());
                    }
                    black_box(&mst);
                    total_duration += start.elapsed();
                }

                total_duration
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_add_records);
criterion_main!(benches);
