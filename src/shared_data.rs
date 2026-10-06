use portable_atomic::{AtomicU128, Ordering};
use std::sync::Mutex;

pub trait SharedData {
    fn write(&self, new_value: f64);
    fn read(&self) -> Option<f64>;
    fn clear(&self);
}

/// Concurrent single-slot mailbox (consume-on-read).
///
/// # Semantics
/// - `write` replaces any unread value (drop-oldest-unread).
/// - `read` consumes exactly one currently present value, or `None`.
/// - Operations linearize on the `AtomicU128` state transition (store / successful CAS / clear store).
/// - A write concurrent with a read may be the value that read returns (reader retries after CAS fail).
/// - `clear` removes whatever is present at its linearization point; a write racing after that can repopulate.
/// - Multiple concurrent readers compete; there is no fairness guarantee.
///
/// # Representation
/// Packed `{ tag:u64, bits:u64 }` in one `portable_atomic::AtomicU128` (`tag == 0` ⇒ empty).
/// Two separate atomics (`bits` + `present`) would tear under concurrent R/W; a sentinel inside
/// one `u64` cannot represent every `f64` (NaN payloads must round-trip by bits).
///
/// 128-bit atomics are lock-free on some targets and may use heavier fallbacks on others —
/// see `BENCH_RESULTS.txt` for numbers from the pinned toolchain on this machine.
pub struct LatestSharedData {
    state: AtomicU128,
}

const EMPTY: u128 = 0;

#[inline]
fn pack(bits: u64) -> u128 {
    (1u128 << 64) | u128::from(bits)
}

#[inline]
fn payload(word: u128) -> Option<u64> {
    if word == EMPTY {
        None
    } else {
        Some(word as u64)
    }
}

impl LatestSharedData {
    pub fn new() -> Self {
        Self {
            state: AtomicU128::new(EMPTY),
        }
    }
}

impl Default for LatestSharedData {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedData for LatestSharedData {
    fn write(&self, new_value: f64) {
        self.state
            .store(pack(new_value.to_bits()), Ordering::Release);
    }

    fn read(&self) -> Option<f64> {
        loop {
            let cur = self.state.load(Ordering::Acquire);
            let bits = payload(cur)?;
            match self
                .state
                .compare_exchange(cur, EMPTY, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Some(f64::from_bits(bits)),
                Err(_) => continue,
            }
        }
    }

    fn clear(&self) {
        self.state.store(EMPTY, Ordering::Release);
    }
}

/// FIFO mailbox: unread values are delivered in write order.
///
/// Contiguous `Vec` + read cursor under one `Mutex`. Poisoning is treated as
/// fatal (`expect`): after a panic while holding the lock the process should
/// not keep serving potentially inconsistent mailbox state.
///
/// On uncontended write-then-drain benches this layout was competitive with
/// `VecDeque` and `crossbeam_queue::SegQueue` on the machine recorded in
/// `BENCH_RESULTS.txt` (hardware-dependent; re-run locally).
pub struct OrderedSharedData {
    inner: Mutex<OrderedInner>,
}

struct OrderedInner {
    data: Vec<f64>,
    head: usize,
}

impl OrderedSharedData {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(OrderedInner {
                data: Vec::with_capacity(64),
                head: 0,
            }),
        }
    }
}

impl Default for OrderedSharedData {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedData for OrderedSharedData {
    fn write(&self, new_value: f64) {
        let mut g = self.inner.lock().expect("lock poisoned");
        if g.head == g.data.len() {
            g.data.clear();
            g.head = 0;
        }
        g.data.push(new_value);
    }

    fn read(&self) -> Option<f64> {
        let mut g = self.inner.lock().expect("lock poisoned");
        if g.head >= g.data.len() {
            return None;
        }
        let value = g.data[g.head];
        g.head += 1;
        // Drop logical contents immediately after a full drain; capacity is kept.
        if g.head == g.data.len() {
            g.data.clear();
            g.head = 0;
        }
        Some(value)
    }

    fn clear(&self) {
        let mut g = self.inner.lock().expect("lock poisoned");
        g.data.clear();
        g.head = 0;
    }
}

#[cfg(test)]
mod tests {
    extern crate test;

    use std::thread;

    use self::test::{black_box, Bencher};
    use crate::shared_data::{LatestSharedData, OrderedSharedData, SharedData};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    #[inline(always)]
    fn seeded_f64_samples(seed: u64, count: usize) -> Vec<f64> {
        let mut rng = StdRng::seed_from_u64(seed);
        (0..count)
            .map(|_| f64::from_bits(rng.gen::<u64>()))
            .collect()
    }

    #[inline(always)]
    fn assert_same_f64_bits(actual: f64, expected: f64) {
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "bit pattern mismatch: actual={actual:?}, expected={expected:?}"
        );
    }

    #[test]
    fn test_latest_read_and_write() {
        let shared = LatestSharedData::new();

        assert_eq!(shared.read(), None);
        shared.write(99.0);
        assert_eq!(shared.read(), Some(99.0));
        assert_eq!(shared.read(), None);
    }

    #[test]
    fn test_ordered_read_and_write() {
        let shared = OrderedSharedData::new();

        assert_eq!(shared.read(), None);
        shared.write(99.0);
        assert_eq!(shared.read(), Some(99.0));
    }

    #[test]
    fn test_latest_reads_in_write_order() {
        let shared = LatestSharedData::new();

        shared.write(7.0);
        shared.write(8.0);
        shared.write(9.0);

        assert_eq!(shared.read(), Some(9.0));
        assert_eq!(shared.read(), None);
    }

    #[test]
    fn test_ordered_reads_in_write_order() {
        let shared = OrderedSharedData::new();

        shared.write(7.0);
        shared.write(8.0);
        shared.write(9.0);

        assert_eq!(shared.read(), Some(7.0));
        assert_eq!(shared.read(), Some(8.0));
        assert_eq!(shared.read(), Some(9.0));
        assert_eq!(shared.read(), None);
    }

    #[test]
    fn test_latest_can_write_on_one_thread_and_read_on_another() {
        // Arrange
        let shared = LatestSharedData::new();

        // Act: write on worker thread.
        thread::scope(|scope| {
            let writer = scope.spawn(|| {
                shared.write(7.0);
                shared.write(8.0);
                shared.write(9.0);
            });
            writer.join().unwrap();
        });

        // Act: read on a different worker thread.
        let (first, second) = thread::scope(|scope| {
            let reader = scope.spawn(|| (shared.read(), shared.read()));
            reader.join().unwrap()
        });

        // Assert
        assert_eq!(first, Some(9.0));
        assert_eq!(second, None);
    }

    #[test]
    fn test_ordered_can_write_on_one_thread_and_read_on_another() {
        // Arrange
        let shared = OrderedSharedData::new();

        // Act: write on worker thread.
        thread::scope(|scope| {
            let writer = scope.spawn(|| {
                shared.write(7.0);
                shared.write(8.0);
                shared.write(9.0);
            });
            writer.join().unwrap();
        });

        // Act: read on a different worker thread.
        let (first, second, third, fourth) = thread::scope(|scope| {
            let reader =
                scope.spawn(|| (shared.read(), shared.read(), shared.read(), shared.read()));
            reader.join().unwrap()
        });

        // Assert
        assert_eq!(first, Some(7.0));
        assert_eq!(second, Some(8.0));
        assert_eq!(third, Some(9.0));
        assert_eq!(fourth, None);
    }

    #[test]
    fn test_latest_clear_wipes_data() {
        let shared = LatestSharedData::new();
        shared.write(42.0);
        assert_eq!(shared.read(), Some(42.0));

        shared.write(99.0);
        shared.clear();

        assert_eq!(shared.read(), None);
    }

    #[test]
    fn test_ordered_clear_wipes_data() {
        let shared = OrderedSharedData::new();
        shared.write(1.0);
        shared.write(2.0);
        shared.write(3.0);

        shared.clear();

        assert_eq!(shared.read(), None);

        shared.write(7.0);
        assert_eq!(shared.read(), Some(7.0));
        assert_eq!(shared.read(), None);
    }

    #[test]
    fn test_handles_special_f64_values_latest() {
        let shared = LatestSharedData::new();
        let special_values = [
            f64::NAN,
            f64::from_bits(0x7ff8_0000_0000_0001), // quiet NaN with payload
            f64::from_bits(0xfff8_0000_0000_0001), // negative quiet NaN payload
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::EPSILON,
            f64::from_bits(1),          // smallest subnormal
            f64::from_bits(1u64 << 63), // negative zero
            0.0,
            -1.0,
            1.0,
            42.5,
        ];

        for value in special_values {
            shared.write(value);
            let actual = shared.read().expect("value should be present");
            assert_same_f64_bits(actual, value);
            assert_eq!(shared.read(), None);
        }
    }

    #[test]
    fn test_handles_special_f64_values_ordered() {
        let shared = OrderedSharedData::new();
        let special_values = [
            f64::NAN,
            f64::from_bits(0x7ff8_0000_0000_0001), // quiet NaN with payload
            f64::from_bits(0xfff8_0000_0000_0001), // negative quiet NaN payload
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::EPSILON,
            f64::from_bits(1),          // smallest subnormal
            f64::from_bits(1u64 << 63), // negative zero
            0.0,
            -1.0,
            1.0,
            42.5,
        ];

        for value in special_values {
            shared.write(value);
        }

        for expected in special_values {
            let actual = shared.read().expect("value should be present");
            assert_same_f64_bits(actual, expected);
        }

        assert_eq!(shared.read(), None);
    }

    #[bench]
    fn bench_no_writes_ten_reads_latest(b: &mut Bencher) {
        b.iter(|| {
            let shared = LatestSharedData::new();
            for _ in 0..10 {
                black_box(shared.read());
            }
        });
    }

    #[bench]
    fn bench_no_writes_ten_reads_ordered(b: &mut Bencher) {
        b.iter(|| {
            let shared = OrderedSharedData::new();
            for _ in 0..10 {
                black_box(shared.read());
            }
        });
    }

    #[bench]
    fn bench_single_write_single_read_latest(b: &mut Bencher) {
        let input = black_box(1273811.0_f64);
        b.iter(|| {
            let shared = LatestSharedData::new();
            shared.write(input);
            black_box(shared.read());
        });
    }

    #[bench]
    fn bench_single_write_single_read_ordered(b: &mut Bencher) {
        let input = black_box(1273811.0_f64);
        b.iter(|| {
            let shared = OrderedSharedData::new();
            shared.write(input);
            black_box(shared.read());
        });
    }

    #[bench]
    fn bench_fifty_writes_single_read_latest(b: &mut Bencher) {
        let inputs = black_box(seeded_f64_samples(1222111357, 50));
        b.iter(|| {
            let shared = LatestSharedData::new();
            for &value in inputs.iter() {
                shared.write(value);
            }
            black_box(shared.read());
        });
    }

    #[bench]
    fn bench_fifty_writes_single_read_ordered(b: &mut Bencher) {
        let inputs = black_box(seeded_f64_samples(1222111357, 50));
        b.iter(|| {
            let shared = OrderedSharedData::new();
            for &value in inputs.iter() {
                shared.write(value);
            }
            black_box(shared.read());
        });
    }

    #[bench]
    fn bench_fifty_writes_read_until_empty_latest(b: &mut Bencher) {
        let inputs = black_box(seeded_f64_samples(45353413, 50));
        b.iter(|| {
            let shared = LatestSharedData::new();
            for &value in inputs.iter() {
                shared.write(value);
            }

            let mut reads = 0usize;
            while let Some(value) = shared.read() {
                black_box(value);
                reads += 1;
            }
            assert_eq!(reads, 1);
            black_box(reads);
        });
    }

    #[bench]
    fn bench_fifty_writes_read_until_empty_ordered(b: &mut Bencher) {
        let inputs = black_box(seeded_f64_samples(45353413, 50));
        b.iter(|| {
            let shared = OrderedSharedData::new();
            for &value in inputs.iter() {
                shared.write(value);
            }

            let mut reads = 0usize;
            while let Some(value) = shared.read() {
                black_box(value);
                reads += 1;
            }
            assert_eq!(reads, 50);
            black_box(reads);
        });
    }

    #[bench]
    fn bench_tenthousand_writes_read_until_empty_latest(b: &mut Bencher) {
        let inputs = black_box(seeded_f64_samples(99123123, 10_000));
        b.iter(|| {
            let shared = LatestSharedData::new();
            for &value in inputs.iter() {
                shared.write(value);
            }

            let mut reads = 0usize;
            while let Some(value) = shared.read() {
                black_box(value);
                reads += 1;
            }
            assert_eq!(reads, 1);
            black_box(reads);
        });
    }

    #[bench]
    fn bench_tenthousand_writes_read_until_empty_ordered(b: &mut Bencher) {
        let inputs = black_box(seeded_f64_samples(99123123, 10_000));
        b.iter(|| {
            let shared = OrderedSharedData::new();
            for &value in inputs.iter() {
                shared.write(value);
            }

            let mut reads = 0usize;
            while let Some(value) = shared.read() {
                black_box(value);
                reads += 1;
            }
            assert_eq!(reads, 10_000);

            black_box(reads);
        });
    }
}
