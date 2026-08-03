//! Version-1 deterministic named random streams.

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const RNG_DERIVATION_LABEL: &[u8] = b"colosseum-rng-v1\0";
pub const RNG_VERSION: u32 = 1;
pub const RNG_ALGORITHM_ID: &str = "chacha12-64-bit-counter-zero-stream-v1";
pub const RNG_DERIVATION_ID: &str = "sha256-colosseum-rng-v1";
pub const RNG_U64_SAMPLING_ID: &str = "little-endian-u64-rejection-v1";

pub mod stream_names {
    pub const OPENING_ORDER: &str = "opening-order";
    pub const SPSA_PERTURBATIONS: &str = "spsa-perturbations";
    pub const BOOTSTRAP_RESAMPLING: &str = "bootstrap-resampling";
    pub const POSITION_ORDER: &str = "position-order";
    pub const WARMUP_SCHEDULING: &str = "warmup-scheduling";
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RngError {
    #[error("stream name must be non-empty ASCII")]
    InvalidStreamName,
    #[error("bounded integer upper bound must be greater than zero")]
    ZeroBound,
    #[error("bootstrap population must not be empty")]
    EmptyPopulation,
}

/// Derive one 256-bit stream key without consuming any other stream.
pub fn derive_stream_seed(master_seed: u64, stream_name: &str) -> Result<[u8; 32], RngError> {
    if stream_name.is_empty() || !stream_name.is_ascii() {
        return Err(RngError::InvalidStreamName);
    }
    let mut hash = Sha256::new();
    hash.update(RNG_DERIVATION_LABEL);
    hash.update(master_seed.to_le_bytes());
    hash.update(stream_name.as_bytes());
    Ok(hash.finalize().into())
}

/// ChaCha12 with a 64-bit block counter and zero 64-bit stream id.
/// Sampling methods are part of RNG version 1 and cannot be replaced by
/// dependency convenience methods without changing `stats_version`.
#[derive(Debug, Clone)]
pub struct NamedRng {
    key: [u32; 8],
    block_counter: u64,
    block: [u8; 64],
    offset: usize,
}

impl NamedRng {
    pub fn new(master_seed: u64, stream_name: &str) -> Result<Self, RngError> {
        Ok(Self::from_stream_seed(derive_stream_seed(
            master_seed,
            stream_name,
        )?))
    }

    #[must_use]
    pub fn from_stream_seed(seed: [u8; 32]) -> Self {
        let mut key = [0_u32; 8];
        for (word, bytes) in key.iter_mut().zip(seed.chunks_exact(4)) {
            *word = u32::from_le_bytes(bytes.try_into().expect("four-byte chunk"));
        }
        Self {
            key,
            block_counter: 0,
            block: [0; 64],
            offset: 64,
        }
    }

    pub fn fill_bytes(&mut self, output: &mut [u8]) {
        let mut written = 0;
        while written < output.len() {
            if self.offset == self.block.len() {
                self.refill();
            }
            let count = (output.len() - written).min(self.block.len() - self.offset);
            output[written..written + count]
                .copy_from_slice(&self.block[self.offset..self.offset + count]);
            self.offset += count;
            written += count;
        }
    }

    /// Position this stream at an absolute byte offset without consuming the
    /// preceding output. This preserves the exact ChaCha12 byte stream and is
    /// used to reconstruct durable iteration-major draw ranges efficiently.
    pub fn seek_bytes(&mut self, byte_offset: u64) {
        self.block_counter = byte_offset / self.block.len() as u64;
        let within_block = (byte_offset % self.block.len() as u64) as usize;
        self.offset = self.block.len();
        if within_block > 0 {
            self.refill();
            self.offset = within_block;
        }
    }

    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        let mut bytes = [0; 8];
        self.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    /// Unbiased `[0, upper)` sampling. Values below `-upper mod upper` are
    /// rejected so the accepted 64-bit range is an exact multiple of `upper`.
    pub fn bounded_u64(&mut self, upper: u64) -> Result<u64, RngError> {
        if upper == 0 {
            return Err(RngError::ZeroBound);
        }
        let threshold = upper.wrapping_neg() % upper;
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return Ok(value % upper);
            }
        }
    }

    #[must_use]
    pub fn rademacher(&mut self) -> i8 {
        if self.bounded_u64(2).expect("two is a non-zero bound") == 0 {
            -1
        } else {
            1
        }
    }

    /// In-place descending Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        for upper in (2..=values.len()).rev() {
            let index = self
                .bounded_u64(upper as u64)
                .expect("shuffle upper bound is at least two") as usize;
            values.swap(upper - 1, index);
        }
    }

    pub fn bootstrap_indices(
        &mut self,
        population: usize,
        samples: usize,
    ) -> Result<Vec<usize>, RngError> {
        if population == 0 {
            return Err(RngError::EmptyPopulation);
        }
        (0..samples)
            .map(|_| {
                self.bounded_u64(population as u64)
                    .map(|value| value as usize)
            })
            .collect()
    }

    fn refill(&mut self) {
        let counter = self.block_counter.to_le_bytes();
        let mut state = [
            0x6170_7865,
            0x3320_646e,
            0x7962_2d32,
            0x6b20_6574,
            self.key[0],
            self.key[1],
            self.key[2],
            self.key[3],
            self.key[4],
            self.key[5],
            self.key[6],
            self.key[7],
            u32::from_le_bytes(counter[0..4].try_into().expect("counter low")),
            u32::from_le_bytes(counter[4..8].try_into().expect("counter high")),
            0,
            0,
        ];
        let initial = state;
        for _ in 0..6 {
            quarter_round(&mut state, 0, 4, 8, 12);
            quarter_round(&mut state, 1, 5, 9, 13);
            quarter_round(&mut state, 2, 6, 10, 14);
            quarter_round(&mut state, 3, 7, 11, 15);
            quarter_round(&mut state, 0, 5, 10, 15);
            quarter_round(&mut state, 1, 6, 11, 12);
            quarter_round(&mut state, 2, 7, 8, 13);
            quarter_round(&mut state, 3, 4, 9, 14);
        }
        for (index, (word, original)) in state.iter().zip(initial).enumerate() {
            self.block[index * 4..index * 4 + 4]
                .copy_from_slice(&word.wrapping_add(original).to_le_bytes());
        }
        self.block_counter = self.block_counter.wrapping_add(1);
        self.offset = 0;
    }
}

fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_stream_names_and_sampling_inputs_are_named() {
        assert_eq!(derive_stream_seed(1, ""), Err(RngError::InvalidStreamName));
        assert_eq!(
            derive_stream_seed(1, "non-ascii-ž"),
            Err(RngError::InvalidStreamName)
        );
        let mut rng = NamedRng::new(1, "test").unwrap();
        assert_eq!(rng.bounded_u64(0), Err(RngError::ZeroBound));
        assert_eq!(rng.bootstrap_indices(0, 1), Err(RngError::EmptyPopulation));
    }

    #[test]
    fn a_new_named_consumer_cannot_shift_an_existing_stream() {
        let mut before = NamedRng::new(99, stream_names::OPENING_ORDER).unwrap();
        let expected: Vec<u64> = (0..8).map(|_| before.next_u64()).collect();

        let mut unrelated = NamedRng::new(99, "future-consumer").unwrap();
        let mut noise = [0; 257];
        unrelated.fill_bytes(&mut noise);

        let mut after = NamedRng::new(99, stream_names::OPENING_ORDER).unwrap();
        let actual: Vec<u64> = (0..8).map(|_| after.next_u64()).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn version_one_golden_vectors_pin_derivation_generator_and_sampling() {
        const MASTER: u64 = 0x0123_4567_89ab_cdef;
        assert_eq!(
            hex(&derive_stream_seed(MASTER, stream_names::OPENING_ORDER).unwrap()),
            "cb674f96baa6f4b214f5ab3db33e23491fb686ba7be898e895068429152ff49d"
        );
        let named_seeds = [
            (
                stream_names::SPSA_PERTURBATIONS,
                "e58ef38a32c2012aa43f0ff3de0a4e1acabf563789afb301c0feff708776dc6f",
            ),
            (
                stream_names::BOOTSTRAP_RESAMPLING,
                "0e647833b843574287ff0bbdc5ec5aebbc89053bcd4b5789bc2e0a50d44465bf",
            ),
            (
                stream_names::POSITION_ORDER,
                "a101cb373d950bcb00fb27699391bedf6930f7d7fda03e407edb0f527ef2a2f1",
            ),
            (
                stream_names::WARMUP_SCHEDULING,
                "320727902f0b893a5de8ba3f8e84fb79aaeda87772f5e895ff09f9c025cc8add",
            ),
        ];
        for (name, expected) in named_seeds {
            assert_eq!(hex(&derive_stream_seed(MASTER, name).unwrap()), expected);
        }

        let mut raw = NamedRng::new(MASTER, stream_names::OPENING_ORDER).unwrap();
        let mut bytes = [0; 64];
        raw.fill_bytes(&mut bytes);
        assert_eq!(
            hex(&bytes),
            "c2198a09daf1a2cd09160d69492bff7c848323cd90f7fa7a916110036907f4553565d341820734b4348b985eb06a2ec5e975f2d492da09119596fc7ef6da02ff"
        );

        let mut bounded = NamedRng::new(MASTER, stream_names::POSITION_ORDER).unwrap();
        let bounds = [2, 3, 10, 1_000, 4_294_967_297];
        let samples: Vec<u64> = bounds
            .into_iter()
            .map(|upper| bounded.bounded_u64(upper).unwrap())
            .collect();
        assert_eq!(samples, [0, 1, 5, 303, 2_357_529_277]);

        let mut shuffled: Vec<u8> = (0..10).collect();
        NamedRng::new(MASTER, stream_names::OPENING_ORDER)
            .unwrap()
            .shuffle(&mut shuffled);
        assert_eq!(shuffled, [3, 6, 0, 5, 9, 1, 7, 4, 8, 2]);

        let mut signs = NamedRng::new(MASTER, stream_names::SPSA_PERTURBATIONS).unwrap();
        let signs: Vec<i8> = (0..16).map(|_| signs.rademacher()).collect();
        assert_eq!(
            signs,
            [-1, 1, 1, -1, 1, -1, 1, -1, -1, 1, -1, -1, 1, 1, 1, 1]
        );

        let mut bootstrap = NamedRng::new(MASTER, stream_names::BOOTSTRAP_RESAMPLING).unwrap();
        assert_eq!(
            bootstrap.bootstrap_indices(7, 12).unwrap(),
            [3, 4, 2, 5, 0, 6, 5, 0, 5, 0, 2, 3]
        );
    }

    #[test]
    fn absolute_seek_reconstructs_the_same_version_one_byte_stream() {
        const MASTER: u64 = 0x0123_4567_89ab_cdef;
        let mut sequential = NamedRng::new(MASTER, stream_names::SPSA_PERTURBATIONS).unwrap();
        let mut expected = [0_u8; 400];
        sequential.fill_bytes(&mut expected);
        for offset in [0_u64, 8, 56, 64, 72, 257] {
            let mut sought = NamedRng::new(MASTER, stream_names::SPSA_PERTURBATIONS).unwrap();
            sought.seek_bytes(offset);
            let mut actual = [0_u8; 32];
            sought.fill_bytes(&mut actual);
            assert_eq!(actual, expected[offset as usize..offset as usize + 32]);
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
