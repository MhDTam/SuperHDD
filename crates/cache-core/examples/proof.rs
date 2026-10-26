use std::{env, fs, path::PathBuf, time::Instant};
use superhdd_cache_core::{FileCache, BLOCK_BYTES};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or("Supply a NEW disposable test directory.")?,
    );
    fs::create_dir(&directory)?;
    let source = directory.join("source.bin");
    let backing = directory.join("ssd-cache.bin");
    let data: Vec<u8> = (0..BLOCK_BYTES * 32)
        .map(|i| ((i * 31 + i / 97) % 251) as u8)
        .collect();
    fs::write(&source, &data)?;
    let mut cache = FileCache::create(&source, &backing, (BLOCK_BYTES * 16) as u64)?;
    let mut output = vec![0; BLOCK_BYTES];
    let start = Instant::now();
    cache.read_at(0, &mut output)?;
    assert_eq!(output, data[..BLOCK_BYTES]);
    let cold_us = start.elapsed().as_micros();
    let source_reads = cache.statistics().source_read_bytes;
    let start = Instant::now();
    cache.read_at(0, &mut output)?;
    let warm_us = start.elapsed().as_micros();
    assert_eq!(output, data[..BLOCK_BYTES]);
    assert_eq!(cache.statistics().source_read_bytes, source_reads);
    cache.write_source(17, &[1, 2, 3, 4])?;
    cache.read_at(0, &mut output)?;
    assert_eq!(&output[17..21], &[1, 2, 3, 4]);
    println!(
        "{}",
        serde_json::json!({"scope":"user-mode disposable-file algorithm harness; NOT transparent or physical-HDD-I/O verification","source":source,"backing":backing,"coldMicros":cold_us,"warmMicros":warm_us,"byteCorrect":true,"sourceApiReadAvoided":true,"writeInvalidationVerified":true,"statistics":cache.statistics()})
    );
    Ok(())
}
