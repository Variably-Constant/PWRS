//! Criterion benches for the Rust-only conversion layer, measured
//! against the fake vtable so no .NET or process crossing is timed.
//! This isolates the marshalling cost pwrs adds on top of a raw call.
//! Publication numbers must come from a quiet box; a control case (the
//! identity clone) is included so cell placement can be read.

use criterion::{criterion_group, criterion_main, Criterion};
use pwrs::testing::{self, Value};
use std::hint::black_box;
use pwrs::{FromPs, IntoPs, PsObject};

fn benches(c: &mut Criterion) {
    let _host = testing::install();

    // The UTF-16 conversions every string parameter and output pays,
    // with the ASCII fast path and the std path side by side.
    c.bench_function("text_to_utf16_ascii", |b| {
        b.iter(|| black_box(pwrs::text::to_utf16(black_box("a moderate string of some length"))));
    });
    c.bench_function("text_from_utf16_ascii", |b| {
        let u: Vec<u16> = "a moderate string of some length".encode_utf16().collect();
        b.iter(|| black_box(pwrs::text::from_utf16(black_box(&u))));
    });

    // The length the output path actually produces. A greeting is nine
    // bytes, and a profile of the pipeline shape shows every call
    // taking the four-byte tail: the sixteen-byte loop above it and the
    // sixty-four-byte ascii scan carry no samples at that size. The
    // moderate cases either side of this one measure a length the hot
    // path does not reach.
    c.bench_function("text_to_utf16_ascii_short", |b| {
        b.iter(|| black_box(pwrs::text::to_utf16(black_box("Hello, x!"))));
    });
    c.bench_function("text_from_utf16_ascii_short", |b| {
        let u: Vec<u16> = "Hello, x!".encode_utf16().collect();
        b.iter(|| black_box(pwrs::text::from_utf16(black_box(&u))));
    });

    // The output path itself, which reuses the buffer on the cmdlet
    // instance and so never allocates. The two cases above allocate,
    // and at these lengths the allocation is most of what they time.
    c.bench_function("text_to_utf16_into_reused_short", |b| {
        let mut buf: Vec<u16> = Vec::with_capacity(64);
        b.iter(|| {
            buf.clear();
            pwrs::text::to_utf16_into(black_box("Hello, x!"), &mut buf);
            black_box(buf.len());
        });
    });
    c.bench_function("text_to_utf16_into_reused", |b| {
        let mut buf: Vec<u16> = Vec::with_capacity(64);
        b.iter(|| {
            buf.clear();
            pwrs::text::to_utf16_into(black_box("a moderate string of some length"), &mut buf);
            black_box(buf.len());
        });
    });
    c.bench_function("text_to_utf16_std", |b| {
        b.iter(|| black_box(black_box("a moderate string of some length").encode_utf16().collect::<Vec<u16>>()));
    });
    c.bench_function("text_from_utf16_std", |b| {
        let u: Vec<u16> = "a moderate string of some length".encode_utf16().collect();
        b.iter(|| black_box(String::from_utf16_lossy(black_box(&u))));
    });

    // The allocation and free that one cmdlet instance costs, at the
    // size of an `Instance<T>` header. An instance pool could remove
    // this pair and nothing else: the default construction, the header,
    // the crossing and the registry read are paid either way, and a
    // reused instance has to be reset or it carries the last
    // invocation's parameters into the next one.
    c.bench_function("instance_alloc_free", |b| {
        b.iter(|| {
            let raw = Box::into_raw(Box::new(black_box([0u64; 6])));
            unsafe { drop(Box::from_raw(raw)) };
        });
    });

    // Control: a bare handle clone/drop, the floor every case pays.
    c.bench_function("control_clone_drop", |b| {
        let o = testing::object(Value::Int(1));
        b.iter(|| {
            let x = black_box(&o).clone();
            black_box(x);
        });
    });

    c.bench_function("i64_into_ps", |b| {
        b.iter(|| {
            let o = black_box(42i64).into_ps().expect("i64");
            black_box(o);
        });
    });

    c.bench_function("i64_from_ps", |b| {
        let o = 42i64.into_ps().expect("i64");
        b.iter(|| black_box(i64::from_ps(black_box(&o)).expect("read")));
    });

    c.bench_function("string_into_ps", |b| {
        b.iter(|| {
            let o = black_box("a moderate string of some length").into_ps().expect("str");
            black_box(o);
        });
    });

    c.bench_function("string_from_ps", |b| {
        let o = "a moderate string of some length".into_ps().expect("str");
        b.iter(|| black_box(String::from_ps(black_box(&o)).expect("read")));
    });

    c.bench_function("vec_i64_into_ps_1k", |b| {
        let data: Vec<i64> = (0..1000).collect();
        b.iter(|| {
            let o = black_box(data.clone()).into_ps().expect("vec");
            black_box(o);
        });
    });

    c.bench_function("vec_i64_from_ps_1k", |b| {
        let o: PsObject = (0..1000i64).collect::<Vec<_>>().into_ps().expect("vec");
        b.iter(|| black_box(<Vec<i64> as FromPs>::from_ps(black_box(&o)).expect("read")));
    });
}

criterion_group!(g, benches);
criterion_main!(g);
