//! Conversions against the fake host.

use crate::testing::{self, Value};
use crate::values::{MAX_DATETIME_TICKS, UNIX_EPOCH_TICKS};
use crate::{DateTimeKind, FromPs, IntoPs, PsArray, PsCredential, PsDateTime, PsGuid, PsMemory, PsObject, PsSecureString, PsTimeSpan};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Holds the fake host for the length of the caller's test. Bind the
/// return value; dropping it immediately releases the host.
fn setup() -> std::sync::MutexGuard<'static, ()> {
    testing::install()
}

#[test]
fn pin_refuses_a_same_width_element_type() {
    let _host = setup();
    // i64 and f64 are both eight bytes, so a width check alone lets
    // the pin through and hands back the bits read as the wrong type.
    let ints = PsObject::from_slice(&[1i64, 2, 3]).expect("i64 array");
    match ints.pin::<f64>() {
        Ok(_) => panic!("an Int64[] must not pin as f64"),
        Err(e) => assert_eq!(e.error_id, "PwrsPinElementType"),
    }
    assert_eq!(ints.pin::<i64>().expect("matching pin").to_vec(), vec![1i64, 2, 3]);
}

#[test]
fn primitives_round_trip() {
    let _host = setup();
    let o = 42i64.into_ps().expect("i64");
    assert_eq!(i64::from_ps(&o).expect("read"), 42);
    assert_eq!(i32::from_ps(&o).expect("read"), 42);
    assert_eq!(u8::from_ps(&o).expect("read"), 42);
    let o = 2.5f64.into_ps().expect("f64");
    assert_eq!(f64::from_ps(&o).expect("read"), 2.5);
    let o = true.into_ps().expect("bool");
    assert!(bool::from_ps(&o).expect("read"));
    let o = "héllo".into_ps().expect("str");
    assert_eq!(String::from_ps(&o).expect("read"), "héllo");
}

#[test]
fn narrowing_reports_overflow() {
    let _host = setup();
    let o = 300i64.into_ps().expect("i64");
    let err = u8::from_ps(&o).expect_err("300 does not fit u8");
    assert_eq!(err.error_id, "PwrsConversionError");
}

#[test]
fn option_and_null() {
    let _host = setup();
    let none: Option<i64> = None;
    let o = none.into_ps().expect("none");
    assert!(o.is_null());
    assert_eq!(<Option<i64> as FromPs>::from_ps(&o).expect("read"), None);
    let some = Some(7i64).into_ps().expect("some");
    assert_eq!(<Option<i64> as FromPs>::from_ps(&some).expect("read"), Some(7));
}

// A Vec enumerates into the pipeline; PsArray writes one object.
const _: () = assert!(<Vec<i64> as IntoPs>::ENUMERATE);
const _: () = assert!(!<PsArray<i64> as IntoPs>::ENUMERATE);

#[test]
fn vec_round_trip() {
    let _host = setup();
    let v = vec![1i64, 2, 3];
    let o = v.into_ps().expect("vec");
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&o).expect("read"), vec![1, 2, 3]);
    let strings = vec!["a".to_string(), "b".to_string()].into_ps().expect("strings");
    assert_eq!(<Vec<String> as FromPs>::from_ps(&strings).expect("read"), vec!["a", "b"]);
}

#[test]
fn string_read_grows_past_the_initial_buffer() {
    let _host = setup();
    let long: String = "x".repeat(1000);
    let o = long.clone().into_ps().expect("long");
    assert_eq!(String::from_ps(&o).expect("read"), long);
}

#[test]
fn handles_are_freed_on_drop() {
    let _host = setup();
    let before = testing::live_handles();
    {
        let a = 1i64.into_ps().expect("a");
        let b = a.clone();
        assert_eq!(testing::live_handles(), before + 2);
        drop(b);
        assert_eq!(testing::live_handles(), before + 1);
    }
    assert_eq!(testing::live_handles(), before);
}

#[test]
fn psobject_notes() {
    let _host = setup();
    let obj = crate::object::new_psobject("Test.Thing");
    crate::object::add_note(&obj, "Name", "n".into_ps().expect("n")).expect("note");
    match testing::value(obj.as_raw()) {
        Value::PsObject(name, props) => {
            assert_eq!(name, "Test.Thing");
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].0, "Name");
        }
        other => panic!("expected a PSObject, got {other:?}"),
    }
}

#[test]
fn a_psobject_property_reads_back_and_a_name_it_lacks_fails() {
    let _host = setup();
    let obj = crate::object::new_psobject("Test.Thing");
    crate::object::add_note(&obj, "Name", "n".into_ps().expect("n")).expect("note");
    let got = crate::object::property(&obj, "Name").expect("property");
    assert_eq!(<String as crate::FromPs>::from_ps(&got).expect("read"), "n");
    assert!(crate::object::property(&obj, "Nope").is_err(), "a name the object does not carry read as a value");
}

#[test]
fn write_enumerates_vec_but_not_psarray() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    ps.write(vec![1i64, 2]).expect("write vec");
    ps.write(PsArray(vec![3i64, 4])).expect("write array");
    let out = testing::take_output();
    assert_eq!(out.len(), 3, "{out:?}");
    assert!(matches!(out[2], Value::Array(_)));
}

#[test]
fn par_map_in_input_order_writes_every_item_in_order() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();

    // Reversed sleeps make the workers finish out of order, so a pass
    // says the reorder buffer ran rather than that the input happened
    // to be produced in sequence.
    let items: Vec<i64> = (0..64).collect();
    ps.par_map(items, crate::Order::Input, |n| {
        if n < 8 {
            std::thread::sleep(std::time::Duration::from_millis(8 - n as u64));
        }
        n * 2
    })
    .expect("par_map");

    let out = testing::take_output();
    assert_eq!(out.len(), 64, "{out:?}");
    for (i, v) in out.iter().enumerate() {
        match v {
            Value::Int(n) => assert_eq!(*n, (i as i64) * 2, "slot {i} out of order"),
            other => panic!("slot {i} is {other:?}"),
        }
    }
}

#[test]
fn par_map_as_ready_writes_every_item_exactly_once() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();

    let items: Vec<i64> = (0..256).collect();
    ps.par_map(items, crate::Order::AsReady, |n| n * 3).expect("par_map");

    let mut seen: Vec<i64> = testing::take_output()
        .into_iter()
        .map(|v| match v {
            Value::Int(n) => n,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    seen.sort_unstable();
    let want: Vec<i64> = (0..256).map(|n| n * 3).collect();
    assert_eq!(seen, want, "every item exactly once, whatever the order");
}

#[test]
fn par_for_each_runs_every_item_and_writes_nothing() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();

    let hits = std::sync::Arc::new(core::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&hits);
    ps.par_for_each((0..512).collect::<Vec<i64>>(), move |_| {
        counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    })
    .expect("par_for_each");

    assert_eq!(hits.load(core::sync::atomic::Ordering::Relaxed), 512);
    assert!(testing::take_output().is_empty(), "par_for_each writes nothing");
}

#[test]
fn a_panicking_worker_is_a_terminating_error_rather_than_a_hang() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();

    // The panicking item is one of many, so the call has to notice a
    // worker that died rather than wait on the results it owed.
    let hushed = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let err = ps
        .par_map((0..64).collect::<Vec<i64>>(), crate::Order::AsReady, |n| {
            if n == 17 {
                panic!("worker refuses 17");
            }
            n
        })
        .expect_err("a panicking worker fails the call");
    std::panic::set_hook(hushed);

    assert_eq!(err.error_id, "PwrsWorkerPanic", "{err:?}");
    assert!(err.terminating, "a worker that died leaves nothing to carry on with");
}

#[test]
fn par_map_over_nothing_is_not_an_error() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    ps.par_map(Vec::<i64>::new(), crate::Order::Input, |n| n).expect("empty");
    assert!(testing::take_output().is_empty());
}

#[test]
fn a_stream_that_is_off_builds_no_text_and_writes_nothing() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let built = core::cell::Cell::new(0u32);
    testing::take_streams();

    testing::set_stream_enabled(pwrs_sys::PS_STREAM_VERBOSE, false);
    let off = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    crate::verbose!(off, "greeting {}", { built.set(built.get() + 1); "x" }).expect("a stream that is off still succeeds");
    assert_eq!(built.get(), 0, "the text was built for a stream nothing reads");
    assert!(testing::take_streams().is_empty(), "a record crossed for a stream that is off");

    // A new pipeline, because the answer is kept for the phase.
    testing::set_stream_enabled(pwrs_sys::PS_STREAM_VERBOSE, true);
    let on = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    crate::verbose!(on, "greeting {}", { built.set(built.get() + 1); "x" }).expect("write");
    assert_eq!(built.get(), 1);
    assert_eq!(testing::take_streams(), vec![(pwrs_sys::PS_STREAM_VERBOSE, "greeting x".to_string())]);
}

#[test]
fn a_stream_answer_is_kept_for_the_phase() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };

    testing::set_stream_enabled(pwrs_sys::PS_STREAM_VERBOSE, true);
    assert!(ps.verbose_enabled());

    testing::set_stream_enabled(pwrs_sys::PS_STREAM_VERBOSE, false);
    testing::set_stream_enabled(pwrs_sys::PS_STREAM_DEBUG, false);
    assert!(ps.verbose_enabled(), "the answer was asked again inside one phase");
    assert!(!ps.debug_enabled(), "a kept verbose answer decided debug too");
}

#[test]
fn errors_reach_the_error_stream_with_their_category() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_errors();
    let e = crate::PsError::new(crate::ErrorCategory::ObjectNotFound, "Missing", "gone");
    ps.write_error(&e).expect("write");
    ps.write_error(&e.terminating()).expect("write");
    let errs = testing::take_errors();
    assert_eq!(errs.len(), 2);
    assert_eq!(errs[0].1, "Missing");
    assert_eq!(errs[0].2, crate::ErrorCategory::ObjectNotFound as u32);
    assert!(!errs[0].3);
    assert!(errs[1].3);
}

#[test]
fn stream_from_thread_forwards_in_order() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    ps.stream_from_thread(|tx| {
        for i in 0..5i64 {
            tx.send(i).expect("send");
        }
    })
    .expect("stream");
    let out = testing::take_output();
    let ints: Vec<i64> = out.iter().map(|v| if let Value::Int(i) = v { *i } else { -1 }).collect();
    assert_eq!(ints, vec![0, 1, 2, 3, 4]);
}

#[test]
fn paths_round_trip_as_strings() {
    let _host = setup();
    let p = std::path::PathBuf::from("C:/tmp/x.txt");
    let o = p.clone().into_ps().expect("path");
    assert_eq!(String::from_ps(&o).expect("read"), "C:/tmp/x.txt");
    assert_eq!(<std::path::PathBuf as FromPs>::from_ps(&o).expect("path back"), p);
    let many = vec![std::path::PathBuf::from("a"), std::path::PathBuf::from("b")].into_ps().expect("vec");
    assert_eq!(<Vec<std::path::PathBuf> as FromPs>::from_ps(&many).expect("read").len(), 2);
}

#[test]
fn a_write_at_its_own_width_costs_a_handle_and_i64_does_not() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    let before = crate::trace::snapshot();
    ps.write(Some(7i32)).expect("some");
    ps.write(2.5f32).expect("f32");
    ps.write(-3isize).expect("isize");
    ps.write(250u8).expect("u8");
    ps.write(None::<i64>).expect("none");
    let after = crate::trace::snapshot();
    // isize is the only one here whose CLR type is what the direct
    // entry builds, so it is the only direct write. The other three
    // scalars each buy their width with one handle, and the None is
    // a null handle.
    assert_eq!(after.direct_writes - before.direct_writes, 1);
    assert_eq!(after.handle_writes - before.handle_writes, 4);
    let out = testing::take_output();
    assert!(
        matches!(out.as_slice(), [Value::Int(7), Value::Float(f), Value::Int(-3), Value::Int(250), Value::Null] if *f == 2.5),
        "{out:?}"
    );
}

#[test]
fn a_scalar_reaches_the_engine_as_its_own_clr_type() {
    let _host = setup();
    // The engine types an operator's answer by its operands' widths,
    // so each of these has to arrive as the type it left as.
    let cases: [(PsObject, pwrs_sys::PsTypeTag); 8] = [
        (7i8.into_ps().expect("i8"), pwrs_sys::PS_TYPE_I8),
        (7i16.into_ps().expect("i16"), pwrs_sys::PS_TYPE_I16),
        (7i32.into_ps().expect("i32"), pwrs_sys::PS_TYPE_I32),
        (7u8.into_ps().expect("u8"), pwrs_sys::PS_TYPE_U8),
        (7u16.into_ps().expect("u16"), pwrs_sys::PS_TYPE_U16),
        (7u32.into_ps().expect("u32"), pwrs_sys::PS_TYPE_U32),
        (2.5f32.into_ps().expect("f32"), pwrs_sys::PS_TYPE_F32),
        (7i64.into_ps().expect("i64"), pwrs_sys::PS_TYPE_I64),
    ];
    for (obj, want) in &cases {
        assert_eq!(obj.type_tag().expect("tag"), *want);
    }
}

#[test]
fn a_decimal_round_trips_through_its_getbits_words() {
    let _host = setup();
    // 123.456: mantissa 123456 with scale 3, and the sign bit clear.
    let d = crate::PsDecimal::from_bits(123_456, 0, 0, 3 << 16);
    assert_eq!(d.scale(), 3);
    assert!(!d.is_negative());
    let obj = d.into_ps().expect("decimal");
    assert_eq!(obj.type_tag().expect("tag"), pwrs_sys::PS_TYPE_DECIMAL);
    assert_eq!(crate::PsDecimal::from_ps(&obj).expect("back"), d);

    let negative = crate::PsDecimal::from_bits(1, 0, 0, i32::MIN | (2 << 16));
    assert!(negative.is_negative());
    assert_eq!(negative.scale(), 2);
}

#[test]
fn memory_order_and_getbits_order_are_a_permutation_of_each_other() {
    // PsDecimalBits is what a pinned Decimal[] element is: the same
    // four words the other way round. Converting twice is identity.
    let d = crate::PsDecimal::from_bits(1, 2, 3, 4 << 16);
    let bits: crate::PsDecimalBits = d.into();
    assert_eq!((bits.lo, bits.mid, bits.hi, bits.flags), (1, 2, 3, 4 << 16));
    assert_eq!(crate::PsDecimal::from(bits), d);
}

#[test]
fn a_datetimeoffset_keeps_its_offset() {
    let _host = setup();
    let v = crate::PsDateTimeOffset::new(637_000_000_000_000_000, -330);
    let obj = v.into_ps().expect("dto");
    let back = crate::PsDateTimeOffset::from_ps(&obj).expect("back");
    assert_eq!(back, v);
    assert_eq!(back.offset_minutes, -330);
    // The same instant on the UTC clock is the offset taken off.
    assert_eq!(back.to_utc_ticks(), v.ticks + 330 * crate::values::TICKS_PER_MINUTE);
}

#[test]
fn an_untagged_type_answers_object_rather_than_guessing() {
    let _host = setup();
    let o = crate::object::new_psobject("Some.Custom.Type");
    assert_eq!(o.type_tag().expect("tag"), pwrs_sys::PS_TYPE_OBJECT);
}

#[test]
fn dates_and_spans_round_trip() {
    let _host = setup();
    let d = PsDateTime::new(638_000_000_000_000_000, DateTimeKind::Local);
    let o = d.into_ps().expect("datetime");
    assert_eq!(PsDateTime::from_ps(&o).expect("read"), d);
    assert_eq!(PsDateTime::utc(1).to_utc().expect("already utc"), PsDateTime::utc(1));
    let too_far = PsDateTime::utc(MAX_DATETIME_TICKS + 1).into_ps().expect_err("past the range");
    assert_eq!(too_far.error_id, "PwrsRuntimeError");
    let span = PsTimeSpan::from_ticks(-15_000_000);
    let o = span.into_ps().expect("timespan");
    assert_eq!(PsTimeSpan::from_ps(&o).expect("read"), span);
    Duration::try_from(span).expect_err("negative span");
    assert_eq!(Duration::try_from(PsTimeSpan::from_ticks(15_000_000)).expect("positive span"), Duration::from_millis(1500));
    assert_eq!(PsTimeSpan::try_from(Duration::from_micros(1)).expect("one microsecond"), PsTimeSpan::from_ticks(10));
}

#[test]
fn system_time_converts_only_as_utc() {
    assert_eq!(PsDateTime::try_from(UNIX_EPOCH).expect("epoch"), PsDateTime::utc(UNIX_EPOCH_TICKS));
    let later = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let d = PsDateTime::try_from(later).expect("later");
    assert_eq!(SystemTime::try_from(d).expect("back"), later);
    let before = UNIX_EPOCH - Duration::from_secs(86_400);
    let d = PsDateTime::try_from(before).expect("before the epoch");
    assert_eq!(d.ticks, UNIX_EPOCH_TICKS - 864_000_000_000);
    assert_eq!(SystemTime::try_from(d).expect("back"), before);
    SystemTime::try_from(PsDateTime::new(d.ticks, DateTimeKind::Local)).expect_err("local kind");
}

#[test]
fn guid_text_and_bytes() {
    let _host = setup();
    let text = "8f1c2b3a-4d5e-6f70-8192-a3b4c5d6e7f8";
    let g: PsGuid = text.parse().expect("parse");
    assert_eq!(g.to_string(), text);
    assert_eq!(g.bytes[..4], [0x3a, 0x2b, 0x1c, 0x8f]);
    assert_eq!(g.bytes[4..6], [0x5e, 0x4d]);
    assert_eq!(g.bytes[6..8], [0x70, 0x6f]);
    assert_eq!(g.bytes[8..], [0x81, 0x92, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7, 0xf8]);
    assert_eq!("{8F1C2B3A-4D5E-6F70-8192-A3B4C5D6E7F8}".parse::<PsGuid>().expect("braced"), g);
    assert_eq!("8f1c2b3a4d5e6f708192a3b4c5d6e7f8".parse::<PsGuid>().expect("plain"), g);
    "8f1c2b3a-4d5e-6f70-8192-a3b4c5d6e7f".parse::<PsGuid>().expect_err("short");
    "8f1c2b3a--4d5e-6f70-8192-a3b4c5d6e7f8".parse::<PsGuid>().expect_err("double hyphen");
    assert_eq!(PsGuid::from_rfc4122(g.to_rfc4122()), g);
    let o = g.into_ps().expect("guid");
    assert_eq!(PsGuid::from_ps(&o).expect("read"), g);
}

#[test]
fn chars_are_one_utf16_unit() {
    let _host = setup();
    let o = 'é'.into_ps().expect("char");
    assert_eq!(char::from_ps(&o).expect("read"), 'é');
    let wide = '😀'.into_ps().expect_err("outside the BMP");
    assert_eq!(wide.error_id, "PwrsConversionError");
    let one = testing::object(Value::Str("x".into()));
    assert_eq!(char::from_ps(&one).expect("one-char string"), 'x');
    let two = testing::object(Value::Str("xy".into()));
    char::from_ps(&two).expect_err("two chars");
    let lone = testing::object(Value::Char(0xD800));
    char::from_ps(&lone).expect_err("surrogate");
}

#[test]
fn secure_strings_reveal_and_credentials_carry_them() {
    let _host = setup();
    let s = PsSecureString::new("hunter2").expect("new");
    assert_eq!(s.len().expect("len"), 7);
    assert!(!s.is_empty().expect("is_empty"));
    assert_eq!(s.reveal().expect("reveal"), "hunter2");
    let empty = PsSecureString::new("").expect("empty");
    assert_eq!(empty.reveal().expect("reveal"), "");
    let long = "x".repeat(65537);
    PsSecureString::new(&long).expect_err("over the limit");
    let plain = testing::object(Value::Str("nope".into()));
    PsSecureString::from_ps(&plain).expect("wraps any handle").reveal().expect_err("not a SecureString");
    let cred = PsCredential::new("ada", "hunter2").expect("credential");
    assert_eq!(cred.user_name, "ada");
    assert_eq!(cred.password.reveal().expect("reveal"), "hunter2");
}

#[test]
fn memory_buffers_are_filled_in_place_and_handed_over() {
    let _host = setup();
    let mut m = PsMemory::<u8>::zeroed(4).expect("alloc");
    m[1] = 7;
    assert_eq!(m.len(), 4);
    let o = m.into_ps().expect("memory");
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&o).expect("read"), vec![0, 7, 0, 0]);
    let copied = PsMemory::from_slice(&[1u8, 2, 3]).expect("alloc");
    assert_eq!(&*copied, &[1, 2, 3]);
    drop(copied);
    let empty = PsMemory::<u8>::zeroed(0).expect("alloc");
    assert!(empty.is_empty());
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&empty.into_ps().expect("empty")).expect("read"), Vec::<i64>::new());
    let wide: PsMemory<i64> = PsMemory::try_from(vec![1i64, 2]).expect("alloc");
    wide.into_ps().expect_err("the fake host wraps bytes only");
}

#[test]
fn each_stream_writer_reaches_its_own_stream() {
    let _host = setup();
    let stopping = std::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_streams();
    ps.verbose("v").expect("verbose");
    ps.debug("d").expect("debug");
    ps.warning("w").expect("warning");
    ps.information("i").expect("information");
    let written = testing::take_streams();
    assert_eq!(
        written,
        vec![
            (pwrs_sys::PS_STREAM_VERBOSE, "v".to_string()),
            (pwrs_sys::PS_STREAM_DEBUG, "d".to_string()),
            (pwrs_sys::PS_STREAM_WARNING, "w".to_string()),
            (pwrs_sys::PS_STREAM_INFORMATION, "i".to_string()),
        ]
    );
}

#[test]
fn null_object_reads_as_empty_collections() {
    let _host = setup();
    let null = PsObject::null();
    assert!(<Vec<i64> as FromPs>::from_ps(&null).expect("vec").is_empty());
    assert_eq!(<Option<String> as FromPs>::from_ps(&null).expect("opt"), None);
}
