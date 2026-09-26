//! Allocations sized by managed input, made through the fallible
//! allocator API.
//!
//! A length read from a managed array or string is whatever the caller
//! handed the shell, and an infallible allocation of it ends the host
//! process when the allocator refuses. These reserve first, so a size
//! the allocator cannot meet becomes a `PwrsOutOfMemory` error record.

use crate::PsResult;

/// An empty vector with room for exactly `len` elements.
pub(crate) fn vec_with_capacity<T>(len: usize) -> PsResult<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(len)?;
    Ok(v)
}

/// A copy of `items`.
pub(crate) fn copy_of<T: Copy>(items: &[T]) -> PsResult<Vec<T>> {
    let mut v = vec_with_capacity(items.len())?;
    v.extend_from_slice(items);
    Ok(v)
}

/// Grows `buf` to `len` elements, filling with `fill`; a `len` at or
/// below the current length leaves it as it is.
pub(crate) fn grow_to<T: Copy>(buf: &mut Vec<T>, len: usize, fill: T) -> PsResult<()> {
    if len > buf.len() {
        buf.try_reserve_exact(len - buf.len())?;
        buf.resize(len, fill);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{copy_of, grow_to, vec_with_capacity};
    use crate::ErrorCategory;

    #[test]
    fn a_size_the_allocator_cannot_meet_is_an_error_record() {
        let e = vec_with_capacity::<u64>(usize::MAX / 8).expect_err("no allocator meets this");
        assert_eq!(e.error_id, "PwrsOutOfMemory");
        assert_eq!(e.category, ErrorCategory::ResourceUnavailable);
        let mut buf = vec![0u16; 4];
        let e = grow_to(&mut buf, usize::MAX / 4, 0).expect_err("no allocator meets this either");
        assert_eq!(e.error_id, "PwrsOutOfMemory");
        assert_eq!(buf.len(), 4, "a refused growth leaves the buffer as it was");
    }

    #[test]
    fn sizes_it_can_meet_behave_like_the_infallible_forms() {
        assert_eq!(copy_of(&[1u8, 2, 3]).expect("small copy"), vec![1, 2, 3]);
        let mut buf = vec![7u16; 2];
        grow_to(&mut buf, 5, 0).expect("small growth");
        assert_eq!(buf, vec![7, 7, 0, 0, 0]);
        grow_to(&mut buf, 3, 9).expect("no shrink");
        assert_eq!(buf.len(), 5);
        assert!(vec_with_capacity::<u32>(16).expect("small").capacity() >= 16);
    }
}
