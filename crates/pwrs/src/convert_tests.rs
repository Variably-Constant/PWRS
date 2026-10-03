//! Conversions against the fake host.

use crate::testing::{self, Value};
use crate::values::{MAX_DATETIME_TICKS, UNIX_EPOCH_TICKS};
use crate::{DateTimeKind, FromPs, IntoPs, PsArray, PsCredential, PsDateTime, PsGuid, PsMemory, PsMemoryView, PsObject, PsRevocation, PsSecureString, PsTimeSpan};
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
fn a_string_past_the_first_read_is_read_in_place_and_unpinned() {
    let _host = setup();
    let before = testing::string_pins_made();
    for (len, pinned) in [(255usize, false), (256, false), (257, true), (1000, true)] {
        let made = testing::string_pins_made();
        let long: String = "x".repeat(len);
        let o = long.clone().into_ps().expect("long");
        assert_eq!(String::from_ps(&o).expect("read"), long);
        assert_eq!(testing::string_pins_made() - made, usize::from(pinned), "{len} units");
    }
    let wide: String = "héllo wörld ✓ 𝄞 ".repeat(40);
    let o = wide.clone().into_ps().expect("wide");
    assert_eq!(String::from_ps(&o).expect("read"), wide);
    assert_eq!(testing::string_pins_made() - before, 3);
    assert_eq!(testing::live_string_pins(), 0);
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
fn an_error_with_details_reaches_the_host_through_the_details_entry() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_errors();
    let plain = crate::PsError::new(crate::ErrorCategory::ObjectNotFound, "Missing", "gone");
    let detailed = crate::PsError::new(crate::ErrorCategory::ObjectNotFound, "Missing", "gone").with_details("looked in /a and /b");
    ps.write_error(&plain).expect("write");
    ps.write_error(&detailed).expect("write");
    ps.write_error(&detailed.terminating()).expect("write");
    let errs = testing::take_errors_with_details();
    assert_eq!(errs.len(), 3);
    assert_eq!(errs[0].0, "gone");
    assert_eq!(errs[0].4, None, "an error without details goes through write_error");
    assert_eq!(errs[1].0, "gone", "the message stays the one line");
    assert_eq!(errs[1].4.as_deref(), Some("looked in /a and /b"));
    assert!(!errs[1].3);
    assert!(errs[2].3, "a terminating error keeps its details");
    assert_eq!(errs[2].4.as_deref(), Some("looked in /a and /b"));
}

#[test]
fn an_entry_point_hands_back_an_exception_when_the_error_carries_details() {
    let _host = setup();
    let plain = crate::PsError::new(crate::ErrorCategory::InvalidArgument, "Bad", "no good");
    let h = unsafe { crate::runtime::error_handle(&plain) };
    let text = testing::value(h);
    drop(unsafe { crate::PsObject::from_raw(h) });
    assert!(matches!(text, testing::Value::Str(ref s) if s == "[Bad] no good"), "{text:?}");
    let detailed = plain.with_details("because");
    let h = unsafe { crate::runtime::error_handle(&detailed) };
    let thrown = testing::value(h);
    drop(unsafe { crate::PsObject::from_raw(h) });
    assert!(matches!(thrown, testing::Value::Exception(ref s) if s == "[Bad] no good\nbecause"), "{thrown:?}");
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

/// The integers written so far, in order; anything else is a failure.
fn written_ints() -> Vec<i64> {
    testing::take_output()
        .into_iter()
        .map(|v| match v {
            Value::Int(n) => n,
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

#[test]
fn write_progress_carries_every_field() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_progress();

    let copying = crate::Progress::new(2, "Copying", "3 of 40 files").with_parent(1).with_current_operation("notes.txt").with_percent(7).with_seconds_remaining(95);
    ps.write_progress(&copying).expect("a record with every field");
    let counting = crate::Progress::new(3, "Scanning", "counting files");
    ps.write_progress(&counting).expect("a processing record with no percentage");
    ps.write_progress(&crate::Progress::new(2, "Copying", "done").completed()).expect("the completing record");
    ps.progress(4, "Hashing", "done", -1).expect("the older entry");

    let seen = testing::take_progress();
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert_eq!(seen[0], copying);
    assert_eq!(seen[1], counting);
    assert!(!seen[1].completed && seen[1].percent_complete == -1, "a negative percent leaves a record processing: {:?}", seen[1]);
    assert!(seen[2].completed);
    assert!(seen[3].completed, "a negative percent through progress() still completes the activity");
}

#[test]
fn a_worker_writes_output_and_every_other_stream() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    testing::take_streams();
    testing::take_errors();
    testing::take_progress();

    ps.stream_from_worker(|w: crate::Worker<i64>| {
        assert!(w.write(1));
        assert!(w.warning("careful"));
        assert!(w.verbose("detail"));
        assert!(w.debug("trace"));
        assert!(w.information("note"));
        assert!(w.write_error(crate::PsError::new(crate::ErrorCategory::ReadError, "Unreadable", "a.txt")));
        assert!(w.write_progress(crate::Progress::new(1, "Reading", "a.txt")));
        assert!(w.write(2));
        assert!(!w.stopping());
    })
    .expect("stream_from_worker");

    assert_eq!(written_ints(), vec![1, 2]);
    assert_eq!(
        testing::take_streams(),
        vec![
            (pwrs_sys::PS_STREAM_WARNING, "careful".to_string()),
            (pwrs_sys::PS_STREAM_VERBOSE, "detail".to_string()),
            (pwrs_sys::PS_STREAM_DEBUG, "trace".to_string()),
            (pwrs_sys::PS_STREAM_INFORMATION, "note".to_string()),
        ]
    );
    assert_eq!(testing::take_errors(), vec![("a.txt".to_string(), "Unreadable".to_string(), crate::ErrorCategory::ReadError as u32, false)]);
    assert_eq!(testing::take_progress(), vec![crate::Progress::new(1, "Reading", "a.txt")]);
}

#[test]
fn a_terminating_error_from_a_worker_ends_the_call_with_it() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    testing::take_errors();

    let err = ps
        .stream_from_worker(|w: crate::Worker<i64>| {
            w.write(1);
            w.write_error(crate::PsError::new(crate::ErrorCategory::InvalidData, "Corrupt", "bad block").terminating());
            w.write(2);
        })
        .expect_err("a terminating error ends the call");
    assert_eq!(err.error_id, "Corrupt");
    assert!(err.terminating);
    assert_eq!(written_ints(), vec![1], "nothing sent after the error is written");
    assert!(testing::take_errors().is_empty(), "the error is the call's to report, not written as it passes");
}

#[test]
fn par_map_with_writes_the_results_in_order_and_what_each_item_reports() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();
    testing::take_streams();

    ps.par_map_with((0..16).collect::<Vec<i64>>(), crate::Order::Input, |n, w| {
        assert!(w.verbose(format!("item {n}")));
        n * 10
    })
    .expect("par_map_with");

    assert_eq!(written_ints(), (0..16).map(|n| n * 10).collect::<Vec<i64>>());
    let mut reported: Vec<String> = testing::take_streams().into_iter().map(|(kind, text)| {
        assert_eq!(kind, pwrs_sys::PS_STREAM_VERBOSE);
        text
    }).collect();
    reported.sort_by_key(|text| text.trim_start_matches("item ").parse::<i64>().expect("an item number"));
    assert_eq!(reported, (0..16).map(|n| format!("item {n}")).collect::<Vec<String>>());
}

/// Items that each run 5 s unless their worker says stop, on two
/// workers, with the pipeline stopped 100 ms in: the call returns well
/// inside the time one item would take, and no item starts after the
/// stop, so at most the two the workers took before it ever started.
/// None at all is also right: on a loaded machine the stop can land
/// before either worker has been scheduled.
fn assert_a_stop_starts_no_further_item(run: impl FnOnce(&crate::Pipeline<'_>, std::sync::Arc<core::sync::atomic::AtomicUsize>) -> crate::PsResult<()>) {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    testing::take_output();

    let started = std::sync::Arc::new(core::sync::atomic::AtomicUsize::new(0));
    let clock = std::time::Instant::now();
    std::thread::scope(|s| {
        s.spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            stopping.store(true, core::sync::atomic::Ordering::Relaxed);
        });
        run(&ps, std::sync::Arc::clone(&started)).expect("a stopped call returns quietly");
    });
    let took = clock.elapsed();

    assert!(took < Duration::from_secs(4), "the stop took {took:?}");
    let n = started.load(core::sync::atomic::Ordering::Relaxed);
    assert!(n <= 2, "{n} items started; each of the two workers takes at most one before the stop and none after");
    testing::take_output();
}

/// One item for [`assert_a_stop_starts_no_further_item`].
fn slow_item(started: &core::sync::atomic::AtomicUsize, w: &crate::Worker) {
    started.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < until && !w.stopping() {
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_stop_ends_par_for_each_with_no_item_started_after_it() {
    assert_a_stop_starts_no_further_item(|ps, started| {
        ps.workers(2)?.par_for_each_with((0..32).collect::<Vec<i64>>(), move |_, w| slow_item(&started, w))
    });
}

#[test]
fn a_stop_ends_par_map_with_no_item_started_after_it() {
    assert_a_stop_starts_no_further_item(|ps, started| {
        ps.workers(2)?.par_map_with((0..32).collect::<Vec<i64>>(), crate::Order::Input, move |n, w| {
            slow_item(&started, w);
            n
        })
    });
}

#[test]
fn workers_caps_the_items_in_flight_at_the_count() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };

    let in_flight = std::sync::Arc::new(core::sync::atomic::AtomicUsize::new(0));
    let peak = std::sync::Arc::new(core::sync::atomic::AtomicUsize::new(0));
    let (flight, high) = (std::sync::Arc::clone(&in_flight), std::sync::Arc::clone(&peak));
    let workers = ps.workers(2).expect("two workers");
    assert_eq!(workers.count(), 2);
    workers
        .par_for_each((0..12).collect::<Vec<i64>>(), move |_| {
            let now = flight.fetch_add(1, core::sync::atomic::Ordering::SeqCst) + 1;
            high.fetch_max(now, core::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            flight.fetch_sub(1, core::sync::atomic::Ordering::SeqCst);
        })
        .expect("par_for_each");
    assert_eq!(peak.load(core::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn workers_refuses_zero() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };
    match ps.workers(0) {
        Ok(_) => panic!("zero workers was accepted"),
        Err(e) => assert_eq!(e.error_id, "PwrsWorkerCount"),
    }
}

#[test]
fn par_for_each_runs_on_the_pool_the_build_selects() {
    let _host = setup();
    let stopping = core::sync::atomic::AtomicBool::new(false);
    let scratch = core::cell::Cell::new(Vec::new());
    let ps = unsafe { crate::Pipeline::new(pwrs_sys::PsHandle::NULL, &stopping, &scratch) };

    let names = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = std::sync::Arc::clone(&names);
    ps.par_for_each((0..256).collect::<Vec<i64>>(), move |_| {
        std::thread::sleep(Duration::from_millis(1));
        seen.lock().expect("names").push(std::thread::current().name().map(str::to_string));
    })
    .expect("par_for_each");

    let names = names.lock().expect("names");
    assert_eq!(names.len(), 256);
    let on_flynnel = names.iter().any(|name| name.as_deref().is_some_and(|n| n.starts_with("flynnel-sched-")));
    assert_eq!(on_flynnel, cfg!(feature = "parallel"), "threads: {names:?}");
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

/// An owner that counts how many of it are alive.
struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Counted {
    fn new(live: &std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Counted {
        live.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Counted(std::sync::Arc::clone(live))
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn a_view_of_borrowed_memory_reads_through_and_its_owner_is_dropped_on_release() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _host = setup();
    let live = std::sync::Arc::new(AtomicUsize::new(0));
    let bytes: Box<[u8]> = vec![5, 6, 7].into_boxed_slice();
    let writable = unsafe { PsMemoryView::writable(bytes.as_ptr().cast_mut(), bytes.len(), Counted::new(&live)) };
    assert_eq!(writable.len(), 3);
    assert_eq!(live.load(Ordering::SeqCst), 1);
    let o = writable.into_ps().expect("a view");
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&o).expect("read"), vec![5, 6, 7]);
    assert_eq!(live.load(Ordering::SeqCst), 0, "the fake host releases a view as it copies it");
    let read_only = unsafe { PsMemoryView::read_only(bytes.as_ptr(), 2, Counted::new(&live)) };
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&read_only.into_ps().expect("a view")).expect("read"), vec![5, 6]);
    let empty = unsafe { PsMemoryView::<u8>::read_only(core::ptr::null(), 0, Counted::new(&live)) };
    assert!(empty.is_empty());
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&empty.into_ps().expect("an empty view")).expect("read"), Vec::<i64>::new());
    assert_eq!(live.load(Ordering::SeqCst), 0);
    let made = crate::testing::views_read_only();
    assert_eq!(made[made.len() - 3..], [false, true, true], "each view crossed as it was asked for");
}

#[test]
fn a_revoked_view_is_refused_and_its_owner_dropped_by_the_module() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _host = setup();
    let live = std::sync::Arc::new(AtomicUsize::new(0));
    let bytes = [1u8, 2];
    let revocation = PsRevocation::new();
    let before = crate::testing::views_read_only().len();
    let tied = unsafe { PsMemoryView::read_only(bytes.as_ptr(), 2, Counted::new(&live)) }.revocable(&revocation);
    assert!(!revocation.is_revoked());
    tied.into_ps().expect("a view not yet revoked");
    revocation.revoke();
    assert!(revocation.clone().is_revoked(), "clones share the flag");
    let refused = unsafe { PsMemoryView::read_only(bytes.as_ptr(), 2, Counted::new(&live)) }.revocable(&revocation);
    let e = refused.into_ps().expect_err("a revoked view");
    assert!(e.message.contains("revoked"), "{}", e.message);
    assert_eq!(live.load(Ordering::SeqCst), 0, "the refused view's owner was dropped once, by the module");
    assert_eq!(crate::testing::views_read_only().len(), before + 1, "the host made only the first view");
    let owned = PsMemory::from_slice(&[9u8]).expect("alloc").read_only().revocable(&revocation);
    owned.into_ps().expect_err("a buffer tied to a revoked flag is refused too");
    let untied = PsMemory::from_slice(&[9u8]).expect("alloc").read_only();
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&untied.into_ps().expect("a read-only buffer")).expect("read"), vec![9]);
    assert_eq!(crate::testing::views_read_only().last(), Some(&true));
}

#[test]
fn an_owner_that_panics_as_it_drops_is_reported_not_unwound() {
    struct Panics;
    impl Drop for Panics {
        fn drop(&mut self) {
            panic!("the owner's drop panicked");
        }
    }
    let _host = setup();
    let bytes = [3u8];
    let view = unsafe { PsMemoryView::read_only(bytes.as_ptr(), 1, Panics) };
    assert_eq!(<Vec<i64> as FromPs>::from_ps(&view.into_ps().expect("a view")).expect("read"), vec![3]);
}

#[test]
fn a_task_settles_once_with_a_value_an_error_or_a_cancellation() {
    use crate::testing::{settled, task_slot, Value};
    use crate::PsTask;
    let _host = setup();
    let before = settled().len();
    let quiet = std::sync::atomic::AtomicU8::new(0);
    let flag = quiet.as_ptr().cast_const();
    let task = unsafe { PsTask::<i64>::from_slot(task_slot("valued", flag)) };
    assert!(!task.is_cancelled());
    quiet.store(1, std::sync::atomic::Ordering::Release);
    assert!(task.is_cancelled(), "the byte the token sets is read with no crossing");
    task.complete(7);
    unsafe { PsTask::<i64>::from_slot(task_slot("failed", flag)) }.fail(crate::PsError::new(crate::ErrorCategory::InvalidArgument, "Nope", "it failed"));
    unsafe { PsTask::<i64>::from_slot(task_slot("canceled", flag)) }.cancel();
    drop(unsafe { PsTask::<i64>::from_slot(task_slot("dropped", flag)) });
    unsafe { PsTask::<()>::from_slot(task_slot("unit", core::ptr::null())) }.complete(());
    unsafe { PsTask::<i64>::from_slot(task_slot("finished", core::ptr::null())) }.finish(Ok(3));
    let s = settled()[before..].to_vec();
    assert_eq!(s.len(), 6, "{s:?}");
    assert!(matches!(&s[0], (Value::Str(n), 0, Value::Int(7)) if n == "valued"), "{:?}", s[0]);
    assert!(matches!(&s[1], (Value::Str(n), 1, Value::Str(e)) if n == "failed" && e.contains("[Nope] it failed")), "{:?}", s[1]);
    assert!(matches!(&s[2], (Value::Str(n), 2, Value::Null) if n == "canceled"), "{:?}", s[2]);
    assert!(matches!(&s[3], (Value::Str(n), 1, Value::Str(e)) if n == "dropped" && e.contains("PwrsTaskDropped")), "{:?}", s[3]);
    assert!(matches!(&s[4], (Value::Str(n), 0, Value::Null) if n == "unit"), "{:?}", s[4]);
    assert!(matches!(&s[5], (Value::Str(n), 0, Value::Int(3)) if n == "finished"), "{:?}", s[5]);
}

#[test]
fn a_task_settles_from_another_thread() {
    use crate::testing::{settled, task_slot, Value};
    use crate::PsTask;
    let _host = setup();
    let before = settled().len();
    let task = unsafe { PsTask::<String>::from_slot(task_slot("threaded", core::ptr::null())) };
    std::thread::spawn(move || task.complete("done".to_string())).join().expect("the worker");
    let s = settled()[before..].to_vec();
    assert!(matches!(&s[..], [(Value::Str(n), 0, Value::Str(v))] if n == "threaded" && v == "done"), "{s:?}");
}

#[test]
fn an_event_carries_its_sender_arguments_and_message_data() {
    use crate::testing::{events, raised, Value};
    let _host = setup();
    let before = raised().len();
    let runspace = events("runspace");
    runspace.raise("Tick", "data").expect("raise");
    runspace.raise_with("Tock", "sender", vec![1i64, 2], 7i64).expect("raise_with");
    runspace.raise_with("Bare", (), (), ()).expect("raise with nothing");
    let r = raised()[before..].to_vec();
    assert_eq!(r.len(), 3, "{r:?}");
    assert!(r.iter().all(|e| matches!(&e.events, Value::Str(m) if m == "runspace")), "{r:?}");
    assert_eq!(r[0].source_identifier, "Tick");
    assert!(matches!((&r[0].sender, &r[0].args, &r[0].message_data), (Value::Null, Value::Null, Value::Str(d)) if d == "data"), "{:?}", r[0]);
    assert_eq!(r[1].source_identifier, "Tock");
    assert!(matches!(&r[1].sender, Value::Str(s) if s == "sender"), "{:?}", r[1]);
    assert!(matches!(&r[1].args, Value::Array(a) if matches!(a.as_slice(), [Value::Int(1), Value::Int(2)])), "{:?}", r[1]);
    assert!(matches!(&r[1].message_data, Value::Int(7)), "{:?}", r[1]);
    assert_eq!(r[2].source_identifier, "Bare");
    assert!(matches!((&r[2].sender, &r[2].args, &r[2].message_data), (Value::Null, Value::Null, Value::Null)), "{:?}", r[2]);
}

#[test]
fn events_raise_in_order_from_another_thread_and_a_refusal_raises_nothing() {
    use crate::testing::{events, raised, Value, FAKE_REFUSED_EVENTS};
    fn shared<T: Send + Sync + Clone>(_: &T) {}
    let _host = setup();
    let before = raised().len();
    let runspace = events("runspace");
    shared(&runspace);
    let worker = runspace.clone();
    std::thread::spawn(move || {
        for n in 0..5i64 {
            worker.raise("Worker", n).expect("a raise from a thread the host never called");
        }
    })
    .join()
    .expect("the worker");
    let numbers: Vec<i64> = raised()[before..]
        .iter()
        .map(|e| match e.message_data {
            Value::Int(n) => n,
            ref other => panic!("an event whose MessageData is {other:?}"),
        })
        .collect();
    assert_eq!(numbers, vec![0, 1, 2, 3, 4]);

    let unconvertible = runspace.raise("Never", '😀').expect_err("a char outside the BMP does not convert");
    assert_eq!(unconvertible.error_id, "PwrsConversionError");
    let refused = events(FAKE_REFUSED_EVENTS).raise("Refused", 1i64).expect_err("the host refuses this raise");
    assert_eq!(refused.error_id, "PwrsRuntimeError");
    assert!(refused.message.contains("refuses every raise"), "{}", refused.message);
    assert_eq!(raised().len(), before + 5, "neither failed raise was recorded");
}

#[test]
fn memory_buffers_and_views_cross_threads() {
    fn send<T: Send>(_: &T) {}
    let bytes = [0u8; 2];
    send(&PsMemory::<u8>::zeroed(1).expect("alloc"));
    send(&unsafe { PsMemoryView::read_only(bytes.as_ptr(), 2, ()) });
    send(&PsRevocation::new());
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
