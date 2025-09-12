use super::bit_math;
use alloy_primitives::U256;

pub fn position(tick: i32) -> (i16, u8) {
    let word_pos = (tick >> 8) as i16;
    let bit_pos = (tick & 0xff) as u8;
    (word_pos, bit_pos)
}

pub fn next_initialized_tick_within_one_word(
    bitmap: U256,
    tick: i32,
    tick_spacing: i32,
    lte: bool,
) -> Option<(i32, bool)> {
    let compressed = tick / tick_spacing;

    if lte {
        let (_word_pos, bit_pos) = position(compressed);
        let mask = (U256::from(1) << bit_pos) - U256::from(1);
        let masked = bitmap & mask;

        if masked != U256::ZERO {
            let most_significant_bit = bit_math::most_significant_bit(masked);
            let next_bit = (bit_pos as i32) - (most_significant_bit as i32);
            let next_tick = (compressed - next_bit) * tick_spacing;
            return Some((next_tick, true));
        }
    } else {
        let (_word_pos, bit_pos) = position(compressed + 1);
        let mask = !((U256::from(1) << bit_pos) - U256::from(1));
        let masked = bitmap & mask;

        if masked != U256::ZERO {
            let least_significant_bit = bit_math::least_significant_bit(masked);
            let next_bit = (least_significant_bit as i32) - (bit_pos as i32);
            let next_tick = (compressed + 1 + next_bit) * tick_spacing;
            return Some((next_tick, true));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Test-only equivalent of Degenbot's flip_tick to set up test scenarios
    fn flip_tick(bitmap: &mut HashMap<i16, U256>, tick: i32) {
        let (word_pos, bit_pos) = position(tick);
        let mask = U256::from(1) << bit_pos;
        let entry = bitmap.entry(word_pos).or_default();
        *entry ^= mask;
    }

    #[test]
    fn test_position() {
        let (word_pos, bit_pos) = position(-230);
        assert_eq!(word_pos, -1);
        assert_eq!(bit_pos, 26);
        
        let (word_pos, bit_pos) = position(230);
        assert_eq!(word_pos, 0);
        assert_eq!(bit_pos, 230);
    }

    #[test]
    fn test_next_initialized_tick_within_one_word_lte_false() {
        let mut bitmap = HashMap::new();
        let initialized_ticks = [-200, -55, -4, 70, 78, 84, 139, 240, 535];
        for &tick in initialized_ticks.iter() {
            flip_tick(&mut bitmap, tick);
        }

        // returns tick to right if at initialized tick
        let (word, _) = position(78);
        let result = next_initialized_tick_within_one_word(bitmap[&word], 78, 1, false);
        assert_eq!(result, Some((84, true)));

        // returns the tick directly to the right
        let (word, _) = position(77);
        let result = next_initialized_tick_within_one_word(bitmap[&word], 77, 1, false);
        assert_eq!(result, Some((78, true)));

        // returns next initialized tick in next word
        let (word, _) = position(-257);
        let result = next_initialized_tick_within_one_word(bitmap[&word], -257, 1, false);
        assert_eq!(result, Some((-200, true)));
    }

    #[test]
    fn test_next_initialized_tick_within_one_word_lte_true() {
        let mut bitmap = HashMap::new();
        let initialized_ticks = [-200, -55, -4, 70, 78, 84, 139, 240, 535];
        for &tick in initialized_ticks.iter() {
            flip_tick(&mut bitmap, tick);
        }

        // returns same tick if initialized
        let (word, _) = position(78);
        let result = next_initialized_tick_within_one_word(bitmap[&word], 78, 1, true);
        assert_eq!(result, Some((78, true)));

        // returns tick to the left
        let (word, _) = position(79);
        let result = next_initialized_tick_within_one_word(bitmap[&word], 79, 1, true);
        assert_eq!(result, Some((78, true)));
    }
}
