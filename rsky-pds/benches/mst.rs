use anyhow::Result;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;
use std::time::Instant;

use lexicon_cid::Cid;
use rsky_pds::{
    common::{get_random_str, ipld::cid_for_cbor, struct_to_cbor, tid::Ticker},
    repo::mst::{util::random_cid, MST},
    storage::SqlRepoReader,
};

// Helper function to generate bulk test data
fn generate_bulk_data_keys(
    count: usize,
    storage: &mut SqlRepoReader,
) -> Result<Vec<(String, Cid)>> {
    let mut entries = Vec::with_capacity(count);
    let mut ticker = Ticker::new();

    for _ in 0..count {
        let key = format!("com.example.record/{}", ticker.next(None).to_string());
        let record = serde_json::json!({ "test": get_random_str() });
        let cid = cid_for_cbor(&record)?;
        let bytes = struct_to_cbor(record)?;
        storage.blocks.set(cid, bytes);
        entries.push((key, cid));
    }
    Ok(entries)
}

fn bench_add_records(c: &mut Criterion) {
    let mut group = c.benchmark_group("mst_operations");
    group.sample_size(20); // Reduce sample size for long-running operations

    for size in [100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("add_records", size), size, |b, &size| {
            // Create fresh MST and data before timing
            let mut storage =
                SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
            let mst = MST::create(storage.clone(), None, None).unwrap();
            let data = generate_bulk_data_keys(size, &mut storage).unwrap();

            b.iter_custom(|iters| {
                let mut total_duration = std::time::Duration::ZERO;

                for _ in 0..iters {
                    let mut mst = mst.clone();
                    let start = Instant::now();

                    // Only time the record additions
                    for (key, cid) in &data {
                        mst = black_box(mst.add(key, *cid, None).unwrap());
                    }
                    black_box(&mst);

                    total_duration += start.elapsed();
                }

                total_duration
            })
        });
    }
    group.finish();
}

fn bench_get_records(c: &mut Criterion) {
    let mut group = c.benchmark_group("mst_operations");
    group.sample_size(20);

    for size in [100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("get_records", size), size, |b, &size| {
            // Setup: Create populated MST before timing
            let mut storage =
                SqlRepoReader::new(None, "did:example:123456789abcdefghi".to_string(), None);
            let mut mst = MST::create(storage.clone(), None, None).unwrap();
            let data = generate_bulk_data_keys(size, &mut storage).unwrap();
            for (key, cid) in &data {
                mst = mst.add(key, *cid, None).unwrap();
            }

            b.iter_custom(|iters| {
                let start = Instant::now();

                for _ in 0..iters {
                    for (key, _) in &data {
                        black_box(mst.get(black_box(key)).unwrap());
                    }
                }

                start.elapsed()
            })
        });
    }
    group.finish();
}

fn bench_update_records(c: &mut Criterion) {
    let mut group = c.benchmark_group("mst_operations");
    group.sample_size(20);

    for size in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("update_records", size),
            size,
            |b, &size| {
                b.iter_custom(|iters| {
                    let mut total_duration = std::time::Duration::ZERO;

                    for _ in 0..iters {
                        // Setup: Create populated MST before timing
                        let mut storage = SqlRepoReader::new(
                            None,
                            "did:example:123456789abcdefghi".to_string(),
                            None,
                        );
                        let mut mst = MST::create(storage.clone(), None, None).unwrap();
                        let data = generate_bulk_data_keys(size, &mut storage).unwrap();
                        for (key, cid) in &data {
                            mst = mst.add(key, *cid, None).unwrap();
                        }

                        let start = Instant::now();

                        // Only time the updates
                        let to_update = data.iter().take(100);
                        for (key, _) in to_update {
                            let new_cid = random_cid(&mut Some(&mut mst.storage)).unwrap();
                            mst = black_box(mst.update(key, new_cid).unwrap());
                        }
                        black_box(&mst);

                        total_duration += start.elapsed();
                    }

                    total_duration
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_add_records,
    bench_get_records,
    bench_update_records
);
criterion_main!(benches);
