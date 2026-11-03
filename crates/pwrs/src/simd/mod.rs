//! Ready-made kernels, each with one body per instruction-set tier and a
//! bit-exact contract: every tier returns what the kernel's scalar form
//! returns, for every input. A kernel runs the widest body for the tier
//! [`crate::cpu::level`] or [`crate::cpu::arm::tier`] reaches, computed
//! once per process and lowered by `PWRS_CPU_MAX`, so `cargo pwrs test
//! --cpu-tiers` runs each body a module reaches on one machine.

pub mod bytes;
pub mod checksums;
pub mod distance;
pub mod filter;
pub mod floats;
pub mod gf256;
pub mod ints;
