use aes_siv::{
    aead::{Aead, OsRng},
    Aes256SivAead, KeyInit, Nonce,
};
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    let key = Aes256SivAead::generate_key(&mut OsRng);
    let cipher = Aes256SivAead::new(&key);
    let nonce = Nonce::default();

    //c.bench_function("encrypt", |b| {
    let mut group = c.benchmark_group("encrypt");
    for size in [1024, 1024 * 1024] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || (0..size).map(|_| rand::random::<u8>()).collect::<Vec<u8>>(),
                |input| cipher.encrypt(&nonce, input.as_ref()).unwrap(),
                BatchSize::SmallInput,
            );
        });
    }
    //});
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
