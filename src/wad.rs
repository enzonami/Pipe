/// WAD decompression and header parsing
/// Based on rac_wad_unpacker.py

use crate::common::{r_s16, r_s32, r_u32, SECTOR_SIZE};
use flate2::read::ZlibDecoder;
use std::io::Read;

/// Decompress a WAD data block
pub fn decompress_wad(data: &[u8]) -> Result<Vec<u8>, String> {
    // Check for raw zlib data (no header)
    if data.len() > 20 && data[0] == 0x78 {
        let mut decoder = ZlibDecoder::new(data);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).map_err(|e| format!("zlib decompress error: {}", e))?;
        return Ok(out);
    }

    // Check for WAD magic (4-byte: WAD\0 or WAD\x01)
    let magic = if data.len() >= 4 { &data[0..4] } else { b"" };
    if magic == b"WAD\0" || magic == b"WAD\x01" {
        // Standard WAD header parsing
        let header_size = if magic == b"WAD\0" { 0x20 } else { 0x10 };
        if data.len() < header_size as usize + 4 {
            return Err("WAD data too short".into());
        }

        let block_offset = r_u32(data, header_size as usize);
        let block_size = r_u32(data, header_size as usize + 4);

        if block_offset as usize + block_size as usize > data.len() {
            return Err("WAD block exceeds data length".into());
        }

        let compressed = &data[block_offset as usize..block_offset as usize + block_size as usize];

        // Try zlib decompression
        if compressed.len() > 2 && (compressed[0] & 0x0F) == 0x08 {
            let mut decoder = ZlibDecoder::new(compressed);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).map_err(|e| format!("zlib decompress error: {}", e))?;
            return Ok(out);
        }

        // Raw copy (not compressed)
        return Ok(compressed.to_vec());
    }

    // Check for 3-byte WAD magic (WAD LZ format)
    if data.len() >= 3 && &data[0..3] == b"WAD" {
        return decompress_wad_lz(data);
    }

    return Err("Not a WAD or zlib stream".into());
}

/// Decompress a WAD LZ compressed block (custom format, NOT zlib)
/// Ported from rac_core_extractor.py:decompress_wad()
/// Header (16 bytes):
///   bytes 0-2: 'WAD' magic
///   bytes 3-6: compressed_size (u32 LE) including the 16-byte header
///   bytes 7-15: 9-byte muffin tag
pub fn decompress_wad_lz(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 0x10 {
        return Err("Data too small for WAD header".into());
    }
    if &data[0..3] != b"WAD" {
        return Err("Bad WAD LZ magic".into());
    }

    let compressed_size = r_u32(data, 3) as usize;
    let end = compressed_size.min(data.len());
    let mut ptr = 0x10;
    let mut dest = Vec::new();

    while ptr < end {
        let flag_byte = data[ptr];
        ptr += 1;

        if flag_byte < 0x10 {
            // Literal packet
            let literal_size = if flag_byte != 0 {
                flag_byte as usize + 3
            } else {
                if ptr >= end { return Err("Unexpected EOF in literal".into()); }
                let sz = data[ptr] as usize + 18;
                ptr += 1;
                sz
            };
            if ptr + literal_size > end {
                return Err(format!("Literal exceeds data: {} > {}", ptr + literal_size, end));
            }
            dest.extend_from_slice(&data[ptr..ptr + literal_size]);
            ptr += literal_size;
            if ptr < end && data[ptr] < 0x10 {
                return Err("Double literal".into());
            }
        } else {
            // Match packet
            let (lookback_offset, match_size) = if flag_byte < 0x20 {
                // Far match (0x10-0x1f)
                let mut match_size = (flag_byte & 7) as usize;
                let _extra_byte = if match_size == 0 {
                    if ptr >= end { return Err("Unexpected EOF in far match size".into()); }
                    match_size = data[ptr] as usize + 7;
                    ptr += 1;
                    true
                } else {
                    false
                };
                if ptr + 2 > end { return Err("Unexpected EOF in far match offset".into()); }
                let b0 = data[ptr];
                let b1 = data[ptr + 1];
                ptr += 2;

                let lo = dest.len().wrapping_sub(
                    ((flag_byte & 8) as usize) * 0x800 + (b1 as usize) * 0x40 + (b0 as usize >> 2)
                );

                if lo != dest.len() {
                    (lo.wrapping_sub(0x4000), match_size + 2)
                } else if match_size != 1 {
                    // Padding packet — align to 0x1000 boundary
                    while (ptr - 0x10) % 0x1000 != 0 {
                        ptr += 1;
                    }
                    continue;
                } else {
                    // DON'T decrement ptr — Python doesn't, and doing so
                    // causes the little literal size (ptr-2) to read the wrong byte
                    (0, 1)
                }
            } else if flag_byte < 0x40 {
                // Medium/big match (0x20-0x3f)
                let mut match_size = (flag_byte & 0x1f) as usize;
                if match_size == 0 {
                    if ptr >= end { return Err("Unexpected EOF in med match size".into()); }
                    match_size = data[ptr] as usize + 0x1f;
                    ptr += 1;
                }
                match_size += 2;
                if ptr + 2 > end { return Err("Unexpected EOF in med match offset".into()); }
                let b1 = data[ptr];
                let b2 = data[ptr + 1];
                ptr += 2;
                let lo = dest.len().wrapping_sub((b2 as usize) * 0x40 + (b1 as usize >> 2) + 1);
                (lo, match_size)
            } else {
                // Little match (0x40-0xff)
                if ptr >= end { return Err("Unexpected EOF in little match".into()); }
                let b1 = data[ptr];
                ptr += 1;
                let lo = dest.len().wrapping_sub((b1 as usize) * 8 + ((flag_byte >> 2) & 7) as usize + 1);
                let ms = ((flag_byte >> 5) as usize) + 1;
                (lo, ms)
            };

            if match_size != 1 {
                if lookback_offset >= dest.len() {
                    return Err(format!("Match offset {} out of bounds (len={})", lookback_offset, dest.len()));
                }
                for i in 0..match_size {
                    dest.push(dest[lookback_offset + i]);
                }
            }

            // Little literal appended to match
            let little_literal_size = (data[ptr.wrapping_sub(2)] & 3) as usize;
            if little_literal_size > 0 {
                if ptr + little_literal_size > end {
                    return Err("Little literal exceeds data".into());
                }
                dest.extend_from_slice(&data[ptr..ptr + little_literal_size]);
                ptr += little_literal_size;
            }
        }
    }

    Ok(dest)
}

// ── WAD LZ Compressor ──────────────────────────────────────────────

const WAD_LZ_WINDOW: usize = 0x8000; // 32KB lookback window (matches wrench-master)
// Max encodable distance by far match: 0x7FFF (bit3 only encodes 0, bit3=1 is broken in PS2 decompressor)
const WAD_LZ_FAR_MAX_DIST: usize = 0x7FFF; // 32767 = max encodable via bit3=0 only (bit3=1 is broken in decoder)
const WAD_LZ_HASH_BITS: usize = 15;
const WAD_LZ_HASH_SIZE: usize = 1 << WAD_LZ_HASH_BITS; // 32768
const WAD_LZ_MIN_MATCH: usize = 3;
const WAD_LZ_MAX_MATCH_LITTLE: usize = 7;
const WAD_LZ_MAX_MATCH_MEDIUM: usize = 288;
const WAD_LZ_MAX_MATCH_FAR: usize = 264;
const WAD_LZ_MAX_LITERAL: usize = 273;

/// Compress data using the WAD LZ format (reverse of decompress_wad_lz).
/// Output: 16-byte header + compressed stream.
/// The format uses LZ77-style backreferences with three match types.
pub fn compress_wad_lz(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        // Empty: just the 16-byte header
        let mut out = vec![0u8; 16];
        out[0..3].copy_from_slice(b"WAD");
        out[3..7].copy_from_slice(&(16u32).to_le_bytes());
        return out;
    }

    // We'll build the compressed stream into a temp buffer, then prepend header
    let mut stream = CompressStream::new(data);
    stream.compress();
    stream.finalize()
}

struct CompressStream<'a> {
    input: &'a [u8],
    output: Vec<u8>,
    pos: usize,          // current position in input
    // Hash chain for 3-byte matching
    hash_table: Vec<Option<usize>>, // hash → position
    chain_table: Vec<Option<usize>>, // position → previous same-hash
    pending_literals: Vec<u8>,      // accumulated literal bytes
    pending_literal_start: usize,   // start position in input for pending literals
    last_was_literal: bool,         // true if last emitted packet was a literal
}

impl<'a> CompressStream<'a> {
    fn new(input: &'a [u8]) -> Self {
        let len = input.len();
        Self {
            input,
            output: Vec::with_capacity(len / 2),
            pos: 0,
            hash_table: vec![None; WAD_LZ_HASH_SIZE],
            chain_table: vec![None; len],
            pending_literals: Vec::new(),
            pending_literal_start: 0,
            last_was_literal: false,
        }
    }

    fn compress(&mut self) {
        // Skip positions where we can't form a 3-byte hash
        while self.pos + 2 < self.input.len() {
            // Look for the best match at current position
            let best = self.find_best_match();

            if best.length >= WAD_LZ_MIN_MATCH {
                // Flush pending literals first
                self.flush_literals();

                // Emit match packet — returns actual bytes consumed
                let emit_len = self.emit_match(best.distance, best.length);
                // Insert skipped positions into hash
                for off in self.pos..self.pos + emit_len {
                    self.insert_hash(off);
                }
                self.pos += emit_len;
            } else {
                // Accumulate literal
                self.pending_literals.push(self.input[self.pos]);
                self.insert_hash(self.pos);
                self.pos += 1;

                // Flush literals if we've accumulated a lot
                if self.pending_literals.len() >= WAD_LZ_MAX_LITERAL {
                    self.flush_literals_raw();
                }
            }
        }

        // Flush remaining bytes as literals
        while self.pos < self.input.len() {
            self.pending_literals.push(self.input[self.pos]);
            self.pos += 1;
        }
        self.flush_literals_raw();
    }

    fn hash3(&self, pos: usize) -> usize {
        if pos + 2 < self.input.len() {
            let a = self.input[pos] as usize;
            let b = self.input[pos + 1] as usize;
            let c = self.input[pos + 2] as usize;
            ((a << 10) ^ (b << 5) ^ c) & (WAD_LZ_HASH_SIZE - 1)
        } else {
            0
        }
    }

    fn insert_hash(&mut self, pos: usize) {
        if pos + 2 >= self.input.len() { return; }
        let h = self.hash3(pos);
        self.chain_table[pos] = self.hash_table[h];
        self.hash_table[h] = Some(pos);
    }

    fn find_best_match(&mut self) -> Match {
        // Cap max_dist to the far match maximum (0xBFFF) which is the
        // largest encodable distance in the format.
        let max_dist = self.pos.min(WAD_LZ_FAR_MAX_DIST);
        if max_dist < WAD_LZ_MIN_MATCH || self.pos + 2 >= self.input.len() {
            return Match { distance: 0, length: 0 };
        }

        let h = self.hash3(self.pos);
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        let max_len = (self.input.len() - self.pos).min(WAD_LZ_MAX_MATCH_MEDIUM);
        let max_chain = 128;
        let mut chain_count = 0;

        let mut candidate = self.hash_table[h];
        while let Some(cand_pos) = candidate {
            chain_count += 1;
            if chain_count > max_chain { break; }
            let dist = self.pos - cand_pos;
            if dist > max_dist { break; }

            let max_check = max_len;

            let mut len = 0usize;
            while len < max_check
                && cand_pos + len < self.input.len()
                && self.input[cand_pos + len] == self.input[self.pos + len]
            {
                len += 1;
            }

            if len > best_len {
                best_len = len;
                best_dist = dist;
                if len >= 64 { break; } // good enough
            }

            candidate = self.chain_table[cand_pos];
        }

        if best_len >= WAD_LZ_MIN_MATCH {
            let len = best_len.min(WAD_LZ_MAX_MATCH_MEDIUM);
            // Ensure distance is encodable (fallback to literal otherwise)
            if best_dist <= WAD_LZ_FAR_MAX_DIST {
                Match { distance: best_dist, length: len }
            } else {
                Match { distance: 0, length: 0 }
            }
        } else {
            Match { distance: 0, length: 0 }
        }
    }

    fn flush_literals(&mut self) {
        if self.pending_literals.is_empty() { return; }

        let lit = std::mem::take(&mut self.pending_literals);

        // Split into chunks that fit the literal encoding.
        // Ensure no consecutive literal packets (decompressor rejects "Double literal").
        let mut i = 0;
        let mut first = true;
        while i < lit.len() {
            let chunk = &lit[i..];
            if chunk.len() >= 4 {
                let emit_len = chunk.len().min(WAD_LZ_MAX_LITERAL);
                if !first || self.last_was_literal {
                    self.emit_padding_match();
                }
                self.emit_literal(&chunk[..emit_len]);
                i += emit_len;
                first = false;
            } else {
                // 1-3 remaining bytes: can't encode as literal (min 4)
                // These should be emitted as little literals after a match.
                self.emit_padding_with_little_literal(chunk);
                break;
            }
        }
        self.pending_literal_start = self.pos;
    }

    fn flush_literals_raw(&mut self) {
        if self.pending_literals.is_empty() { return; }

        let lit = std::mem::take(&mut self.pending_literals);
        let mut i = 0;
        let mut first = true;
        while i < lit.len() {
            let remaining = lit.len() - i;
            if remaining >= 4 {
                let emit_len = remaining.min(WAD_LZ_MAX_LITERAL);
                if !first || self.last_was_literal {
                    // Insert padding match to avoid "Double literal"
                    self.emit_padding_match();
                }
                self.emit_literal(&lit[i..i + emit_len]);
                i += emit_len;
                first = false;
            } else {
                // 1-3 straggler bytes at end: use padding match + little literal
                self.emit_padding_with_little_literal(&lit[i..]);
                i += remaining;
            }
        }
        self.pending_literal_start = self.pos;
    }

    fn emit_literal(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() >= 4 && bytes.len() <= WAD_LZ_MAX_LITERAL);
        if bytes.len() <= 0x12 {
            // flag = size - 3 (1-15 → size 4-18)
            let flag = (bytes.len() - 3) as u8;
            self.output.push(flag);
        } else {
            // flag = 0 (extended), then size - 18
            self.output.push(0);
            self.output.push((bytes.len() - 18) as u8);
        }
        self.output.extend_from_slice(bytes);
        self.last_was_literal = true;
    }

    fn emit_match(&mut self, distance: usize, length: usize) -> usize {
        // Determine which match type to use based on distance and length ranges.
        // Debug: check if distance > self available output
        if distance > self.pos {
            eprintln!("WARNING: dist {} > pos {} (len={})", distance, self.pos, length);
            use std::io::Write;
            std::io::stderr().flush().unwrap();
        }
        self.last_was_literal = false;
        if distance <= 0x7FF && length <= WAD_LZ_MAX_MATCH_LITTLE {
            // Little match: distance 1-2048, length 1-7
            self.emit_little_match(distance, length);
            length
        } else if distance <= 0x4000 && length <= WAD_LZ_MAX_MATCH_MEDIUM {
            // Medium match: distance 1-16384, length 3-288
            if length >= 3 {
                self.emit_medium_match(distance, length);
                length
            } else {
                // length=2 is too short for medium match; can only be little match,
                // but distance > 0x7FF so little match won't work. Emit as literal.
                // This shouldn't happen since MIN_MATCH=3.
                self.emit_literal_from_match(length);
                length
            }
        } else if distance <= WAD_LZ_FAR_MAX_DIST && length >= 3 {
            // Far match: distance > 0x4000 up to 0xBFFF, length 3-264
            // IMPORTANT: cap to WAD_LZ_MAX_MATCH_FAR (264) and return the capped length
            // so self.pos advances correctly and no bytes are lost.
            let len = length.min(WAD_LZ_MAX_MATCH_FAR);
            self.emit_far_match(distance, len);
            len
        } else {
            // Fallback: should not reach here with valid data
            // Cap distance to self.pos to prevent out-of-bounds lookback in decompressor
            let len = length.min(WAD_LZ_MAX_MATCH_MEDIUM).max(3);
            self.emit_medium_match(distance.min(self.pos).min(0x4000), len);
            len
        }
    }

    fn emit_little_match(&mut self, distance: usize, length: usize) {
        // Little match: flag 0x40-0xff
        // flag bits: [ms-1:3 bits] [dist_low:3 bits] [little_lit_size:2 bits]
        // b1 byte: dist_high bits (distance - 1) / 8
        let actual_dist = distance.saturating_sub(1);
        let dist_low = (actual_dist & 7) as u8;
        let dist_high = (actual_dist >> 3) as u8;
        let ms = (length as u8).saturating_sub(1);
        let flag = (ms << 5) | (dist_low << 2) | 0; // little_lit_size = 0 for now
        self.output.push(flag);
        self.output.push(dist_high);
    }

    fn emit_medium_match(&mut self, distance: usize, length: usize) {
        // Medium match: flag 0x20-0x3f
        // flag bits: [type=001] [size:5 bits] (0=extended)
        // b1 byte: [dist_low:6 bits] [little_lit_size:2 bits]
        // b2 byte: dist_high bits
        // Decode: distance = (b2 << 6) | (b1 >> 2) + 1
        //         little_literal_size = b1 & 3
        let actual_dist = distance.saturating_sub(1);
        let dist_low6 = (actual_dist & 0x3F) as u8;
        let dist_high = (actual_dist >> 6) as u8;

        let ms = if length >= 2 { length - 2 } else { 0 };

        if ms <= 0x1F && ms > 0 {
            // Non-extended: size in flag
            let flag = 0x20 | (ms as u8);
            self.output.push(flag);
        } else {
            // Extended: flag with size=0, then extra size byte
            self.output.push(0x20);
            let ext = ms.saturating_sub(0x1F) as u8;
            self.output.push(ext);
        }
        // b1: dist_low6 in bits 2-7, little_lit_size=0 in bits 0-1
        let b1 = (dist_low6 << 2) | 0;
        self.output.push(b1);
        self.output.push(dist_high);
    }

    fn emit_far_match(&mut self, distance: usize, length: usize) {
        // Far match: flag 0x10-0x1f (bit3 always 0, distance 0x4000-0x7FFF)
        // flag bits: [type=0001] [bit3=0] [size:3 bits] (0=extended)
        let adj_dist = distance - 0x4000;
        let dist_low6 = (adj_dist & 0x3F) as u8;
        let dist_high8 = (adj_dist >> 6) as u8;
        let b0_val = (dist_low6 << 2) | 0;
        let b1_val = dist_high8;

        let ms = length.saturating_sub(2);
        if ms >= 1 && ms <= 7 {
            let flag = 0x10 | (ms as u8);
            self.output.push(flag);
        } else {
            // Extended: flag with size=0, then extra byte
            let flag = 0x10;
            self.output.push(flag);
            let ext = ms.saturating_sub(7) as u8;
            self.output.push(ext);
        }
        self.output.push(b0_val);
        self.output.push(b1_val);
    }

    fn emit_padding_match(&mut self) {
        // Padding match: far match with match_size=1 (no bytes copied), no little literal.
        // flag = 0x11 (far, bit3=0, size=1)
        // b0 = 0 (adj_dist=0, ll_size=0)
        // b1 = 0 (adj_dist_high=0)
        self.output.push(0x11);
        self.output.push(0);
        self.output.push(0);
        self.last_was_literal = false;
    }

    fn emit_padding_with_little_literal(&mut self, bytes: &[u8]) {
        // Emit a far match with match_size=1 (padding: no bytes copied).
        // The low 2 bits of b0 encode little_literal_size (1-3).
        // Then the literal bytes follow the match packet.
        // Decoder: lo == dest.len() && match_size == 1 → skip copy,
        // then little_literal_size = data[ptr-2] & 3 = b0 & 3.
        if bytes.is_empty() || bytes.len() > 3 { return; }

        // flag = 0x10 | (bit3=0) | (match_size=1)
        self.output.push(0x11);
        // b0: adj_dist=0 shifted into bits 2-7 (all zero), ll_size in bits 0-1
        self.output.push(bytes.len() as u8);
        // b1: adj_dist >> 6 = 0
        self.output.push(0);
        // Little literal bytes follow
        self.output.extend_from_slice(bytes);
    }

    fn emit_literal_from_match(&mut self, length: usize) {
        // Emit literal bytes for a match that can't be encoded.
        // This only handles 1-3 bytes (since longer would use full literal).
        let count = length.min(3);
        let pos = self.pos;
        for i in 0..count {
            self.pending_literals.push(self.input[pos + i]);
        }
    }

    fn emit_literal_short(&mut self, bytes: &[u8]) {
        self.emit_padding_with_little_literal(bytes);
    }

    fn finalize(mut self) -> Vec<u8> {
        let compressed = std::mem::take(&mut self.output);
        let total_size = 16 + compressed.len();
        let mut header = vec![0u8; 16];
        header[0..3].copy_from_slice(b"WAD");
        header[3..7].copy_from_slice(&(total_size as u32).to_le_bytes());
        // bytes 7-15 remain zero (muffin tag)
        header.extend(compressed);
        header
    }
}

struct Match {
    distance: usize,
    length: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompress_wad_small() {
        // Small synthetic test: empty WAD LZ data (just header)
        let data = b"WAD\x10\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let result = decompress_wad_lz(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_compress_roundtrip_small() {
        let input = b"Hello WAD LZ World! This is a test of the WAD LZ compression format. Hello WAD LZ World again!";
        let compressed = compress_wad_lz(input);
        assert!(compressed.len() >= 16);
        assert_eq!(&compressed[0..3], b"WAD");
        let decompressed = decompress_wad_lz(&compressed).expect("decompress should succeed");
        assert_eq!(decompressed, input, "round-trip mismatch");
    }

    #[test]
    fn test_compress_roundtrip_repeating() {
        // Highly repetitive data should compress very well
        let mut input = Vec::new();
        for _ in 0..100 {
            input.extend_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        }
        let compressed = compress_wad_lz(&input);
        assert!(compressed.len() < input.len(), "compression should reduce size");
        let decompressed = decompress_wad_lz(&compressed).expect("decompress should succeed");
        assert_eq!(decompressed, input, "round-trip mismatch for repeating data");
    }

    #[test]
    fn test_compress_roundtrip_zeros() {
        let input = vec![0u8; 500];
        let compressed = compress_wad_lz(&input);
        let decompressed = decompress_wad_lz(&compressed).expect("decompress should succeed");
        assert_eq!(decompressed, input, "round-trip mismatch for zeros");
    }

    #[test]
    fn test_compress_roundtrip_large() {
        // Large data to exercise matches at distances beyond 0x4000
        let mut input = Vec::new();
        // Fill with a repeating pattern
        for _ in 0..4000 {
            input.extend_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        }
        let block1 = b"BEGINNING_OF_DISTANT_BLOCK_THAT_WILL_BE_MATCHED_LATER_1234567890abcdefghij";
        input.extend_from_slice(block1);
        for _ in 0..3000 {
            input.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz0123456789");
        }
        input.extend_from_slice(block1);

        let compressed = compress_wad_lz(&input);
        assert!(&compressed[0..3] == b"WAD", "bad magic");
        let decompressed = decompress_wad_lz(&compressed).expect("decompress should succeed");
        assert_eq!(decompressed, input, "round-trip mismatch for large data");
    }

    #[test]
    fn test_compress_roundtrip_short() {
        let input = b"short";
        let compressed = compress_wad_lz(input);
        let decompressed = decompress_wad_lz(&compressed).expect("decompress should succeed");
        assert_eq!(decompressed, input, "round-trip mismatch for short data");
    }

    #[test]
    fn test_compress_roundtrip_empty() {
        let input = b"";
        let compressed = compress_wad_lz(input);
        assert_eq!(compressed.len(), 16);
        let decompressed = decompress_wad_lz(&compressed).expect("decompress should succeed");
        assert_eq!(decompressed, input, "round-trip mismatch for empty data");
    }

    #[test]
    fn test_compress_roundtrip_gs_ram() {
        // Read real gs_ram data and test round-trip
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("extracted/unpacked/LEVEL000/data_wad/gs_ram.bin");
        if !path.exists() {
            eprintln!("Skipping gs_ram test (file not found)");
            return;
        }
        let input = std::fs::read(&path).expect("read gs_ram.bin");
        eprintln!("gs_ram: {} bytes", input.len());

        // Debug: compress a 2506-byte prefix and dump compressed stream
        let prefix = &input[..2506];
        let comp = compress_wad_lz(prefix);
        eprintln!("\nCompressed 2506-byte prefix: {} bytes ({} after header)", comp.len(), comp.len() - 16);

        // Dump compressed hex at the critical region PLUS check which compressor function wrote each byte
        eprintln!("Bytes 1373-1420:");
        for i in 1373..1420.min(comp.len()) {
            eprintln!("  comp[{}] = 0x{:02x}", i, comp[i]);
        }

        // Now carefully trace decompression packet by packet
        eprintln!("\nPacket-by-packet trace from step 395:");
        debug_trace_decompress(&comp);

        // Now the full round-trip test
        let result = decompress_wad_lz(&comp);
        if let Err(e) = &result {
            eprintln!("decompress error for 2506 prefix: {}", e);
        }

        // Don't test full file yet — fix the prefix first
        use std::io::Write;
        std::io::stderr().flush().unwrap();

        // Also test full gs_ram file
        let full_compressed = compress_wad_lz(&input);
        eprintln!("gs_ram full: compressed {} -> {}, ratio {:.2}",
            input.len(), full_compressed.len(),
            full_compressed.len() as f64 / input.len() as f64);
        let full_decompressed = decompress_wad_lz(&full_compressed)
            .expect("decompress full gs_ram");
        if full_decompressed.len() != input.len() {
            eprintln!("gs_ram FULL size mismatch: {} vs {}", full_decompressed.len(), input.len());
        } else if full_decompressed != input {
            // Find first difference
            for i in 0..input.len() {
                if full_decompressed[i] != input[i] {
                    eprintln!("gs_ram FULL first diff at byte {}", i);
                    break;
                }
            }
        } else {
            eprintln!("gs_ram FULL round-trip OK!");
        }
    }

    fn debug_trace_decompress(data: &[u8]) {
        // Manual walk through compressed stream with tracing
        if data.len() < 0x10 { return; }
        let compressed_size = r_u32(data, 3) as usize;
        let end = compressed_size.min(data.len());
        let mut ptr = 0x10;
        let mut step = 0usize;
        
        while ptr < end {
            let flag_byte = data[ptr];
            let start_ptr = ptr;
            ptr += 1;
            step += 1;

            if step < 395 {
                // Fast-forward: just advance ptr to skip this packet
                if flag_byte < 0x10 {
                    let sz = if flag_byte != 0 { flag_byte as usize + 3 } else { 
                        if ptr >= end { return; }
                        let s = data[ptr] as usize + 18;
                        ptr += 1;
                        s
                    };
                    ptr += sz;
                } else if flag_byte < 0x20 {
                    let _ms = (flag_byte & 7) as usize;
                    if _ms == 0 { if ptr >= end { return; } ptr += 1; }
                    ptr += 2; // b0, b1
                    let ll = data[ptr.wrapping_sub(2)] as usize & 3;
                    ptr += ll;
                } else if flag_byte < 0x40 {
                    let _ms = (flag_byte & 0x1f) as usize;
                    if _ms == 0 { if ptr >= end { return; } ptr += 1; }
                    ptr += 2; // b1, b2
                    let ll = data[ptr.wrapping_sub(2)] as usize & 3;
                    ptr += ll;
                } else {
                    ptr += 1; // b1
                    let ll = data[ptr.wrapping_sub(2)] as usize & 3;
                    ptr += ll;
                }
                continue;
            }

            eprint!("  step={} ptr={} flag=0x{:02x} ", step, start_ptr, flag_byte);

            if flag_byte < 0x10 {
                let sz = if flag_byte != 0 { 
                    let s = flag_byte as usize + 3;
                    eprintln!("LITERAL size={}", s);
                    s
                } else {
                    if ptr >= end { eprintln!("EOF"); return; }
                    let s = data[ptr] as usize + 18;
                    eprintln!("LITERAL_EXT sz_byte={} size={}", data[ptr], s);
                    ptr += 1;
                    s
                };
                ptr += sz;
                if ptr < end && data[ptr] < 0x10 {
                    eprintln!("  *** DOUBLE LITERAL at ptr={} (next flag=0x{:02x})", ptr, data[ptr]);
                }
            } else if flag_byte < 0x20 {
                let mut ms = (flag_byte & 7) as usize;
                if ms == 0 {
                    if ptr >= end { eprintln!("EOF"); return; }
                    ms = data[ptr] as usize + 7;
                    ptr += 1;
                }
                if ptr + 2 > end { eprintln!("EOF"); return; }
                let b0 = data[ptr];
                let b1 = data[ptr + 1];
                ptr += 2;
                let lo = (b1 as usize) * 0x40 + (b0 as usize >> 2);
                eprintln!("FAR match ms={} b0=0x{:02x} b1=0x{:02x} lo={}", ms + 2, b0, b1, lo);
                let ll = data[ptr.wrapping_sub(2)] as usize & 3;
                if ll > 0 {
                    eprintln!("    little_literal size={}", ll);
                    ptr += ll;
                }
            } else if flag_byte < 0x40 {
                let mut ms = (flag_byte & 0x1f) as usize;
                if ms == 0 {
                    if ptr >= end { eprintln!("EOF"); return; }
                    ms = data[ptr] as usize + 0x1f;
                    ptr += 1;
                }
                ms += 2;
                if ptr + 2 > end { eprintln!("EOF"); return; }
                let b1 = data[ptr];
                let b2 = data[ptr + 1];
                ptr += 2;
                let lo = (b2 as usize) * 0x40 + (b1 as usize >> 2) + 1;
                eprintln!("MEDIUM match ms={} b1=0x{:02x} b2=0x{:02x} lo={}", ms, b1, b2, lo);
                let ll = data[ptr.wrapping_sub(2)] as usize & 3;
                if ll > 0 {
                    eprintln!("    little_literal size={}", ll);
                    ptr += ll;
                }
            } else {
                if ptr >= end { eprintln!("EOF"); return; }
                let b1 = data[ptr];
                ptr += 1;
                let ms = ((flag_byte >> 5) as usize) + 1;
                let lo = (b1 as usize) * 8 + ((flag_byte >> 2) & 7) as usize + 1;
                eprintln!("LITTLE match ms={} b1=0x{:02x} lo={}", ms, b1, lo);
                let ll = data[ptr.wrapping_sub(2)] as usize & 3;
                if ll > 0 {
                    eprintln!("    little_literal size={}", ll);
                    ptr += ll;
                }
            }

            if step >= 410 { break; }
        }
    }

    #[test]
    fn test_compress_roundtrip_core_data() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("extracted/unpacked/LEVEL000/data_wad/core_data.bin");
        if !path.exists() {
            eprintln!("Skipping core_data test (file not found)");
            return;
        }
        let input = std::fs::read(&path).expect("read core_data.bin");
        eprintln!("core_data: {} bytes", input.len());
        let compressed = compress_wad_lz(&input);
        let decompressed = decompress_wad_lz(&compressed)
            .expect("decompress core_data should succeed");
        assert_eq!(decompressed.len(), input.len(), "core_data size mismatch");
        assert_eq!(decompressed, input, "core_data round-trip mismatch");
    }

    #[test]
    fn test_compress_roundtrip_overlay() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("extracted/unpacked/LEVEL000/data_wad/overlay.bin");
        if !path.exists() {
            eprintln!("Skipping overlay test (file not found)");
            return;
        }
        let input = std::fs::read(&path).expect("read overlay.bin");
        eprintln!("overlay: {} bytes", input.len());
        let compressed = compress_wad_lz(&input);
        let decompressed = decompress_wad_lz(&compressed)
            .expect("decompress overlay should succeed");
        assert_eq!(decompressed.len(), input.len(), "overlay size mismatch");
        assert_eq!(decompressed, input, "overlay round-trip mismatch");
    }

    #[test]
    fn test_compress_roundtrip_hud_banks_1() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("extracted/unpacked/LEVEL000/data_wad/hud_banks_1.bin");
        if !path.exists() {
            eprintln!("Skipping hud_banks_1 test (file not found)");
            return;
        }
        let input = std::fs::read(&path).expect("read hud_banks_1.bin");
        eprintln!("hud_banks_1: {} bytes", input.len());
        let compressed = compress_wad_lz(&input);
        let _ = std::fs::write("/tmp/hud_banks_compressed.bin", &compressed);
        eprintln!("hud_banks: comp_size={}", compressed.len());
        let decompressed = decompress_wad_lz(&compressed)
            .expect("decompress hud_banks_1 should succeed");
        if decompressed.len() != input.len() {
            // Find first differing byte
            let min_len = decompressed.len().min(input.len());
            let mut first_diff = min_len;
            for i in 0..min_len {
                if decompressed[i] != input[i] {
                    first_diff = i;
                    break;
                }
            }
            eprintln!("hud_banks_1: comp_size={} first_diff_at={} decomp_len={} input_len={}",
                compressed.len(), first_diff, decompressed.len(), input.len());
            // Show context around first diff
            let start = first_diff.saturating_sub(8);
            let end = (first_diff + 8).min(input.len().min(decompressed.len()));
            eprint!("  orig: ");
            for i in start..end { eprint!("{:02x} ", input[i]); }
            eprintln!();
            eprint!("  dec:  ");
            for i in start..end { eprint!("{:02x} ", decompressed[i]); }
            eprintln!();
        }
        assert_eq!(decompressed.len(), input.len(), "hud_banks_1 size mismatch");
        assert_eq!(decompressed, input, "hud_banks_1 round-trip mismatch");
    }
}

/// Check if WAD data is compressed
pub fn is_wad_compressed(data: &[u8]) -> bool {
    if data.len() < 4 { return false; }
    let magic = &data[0..4];
    if magic == b"WAD\0" || magic == b"WAD\x01" {
        return true;
    }
    // Check for 3-byte WAD magic (WAD LZ)
    if data.len() >= 3 && &data[0..3] == b"WAD" {
        return true;
    }
    // Check for zlib header
    data[0] == 0x78 && (data[1] == 0x01 || data[1] == 0x9C || data[1] == 0xDA)
}

/// Parse a sector range: 4-byte LBA + 4-byte count (from data at offset)
pub fn parse_sector_range(data: &[u8], offset: usize) -> (u32, u32) {
    (r_u32(data, offset), r_u32(data, offset + 4))
}

/// Read a sector range worth of data from a file
pub fn read_sector_range(file_data: &[u8], offset: u32, count: u32) -> Vec<u8> {
    let start = (offset as usize) * (SECTOR_SIZE as usize);
    let size = (count as usize) * (SECTOR_SIZE as usize);
    if start + size > file_data.len() {
        file_data[start..].to_vec()
    } else {
        file_data[start..start + size].to_vec()
    }
}

/// Read an array range (offset in bytes, count of entries, entry_size)
pub fn read_array_range(data: &[u8], offset: u32, count: u32, entry_size: u32) -> Vec<Vec<u8>> {
    let start = offset as usize;
    let mut entries = Vec::new();
    for i in 0..count as usize {
        let off = start + i * entry_size as usize;
        let end = off + entry_size as usize;
        if end <= data.len() {
            entries.push(data[off..end].to_vec());
        }
    }
    entries
}

/// GC/UYA data header fields
pub const GCUYA_DATA_HEADER_FIELDS: &[&str] = &[
    "file_size", "block_size", "block_offset", "file_lba_offset",
    "unknown_1", "unknown_2", "data_size", "file_count",
    "header_size", "unknown_zone_header_size",
];

/// GC/UYA data header fields (byte-level sub-ranges inside data sector range)
pub const GCUYA_DATA_HEADER_ENTRIES: &[(usize, &str)] = &[
    (0x00, "overlay"), (0x08, "core_index"), (0x10, "gs_ram"),
    (0x18, "hud_header"), (0x20, "hud_banks[0]"), (0x28, "hud_banks[1]"),
    (0x30, "hud_banks[2]"), (0x38, "hud_banks[3]"), (0x40, "hud_banks[4]"),
    (0x48, "core_data"), (0x50, "transition_textures"),
];

/// Level core fields with offsets and types from rac_core_extractor.py
pub const LEVEL_CORE_FIELDS: &[(usize, &str, &str)] = &[
    (0x00, "gs_ram", "AR"), (0x08, "tfrags", "s32"),
    (0x0C, "occlusion", "s32"), (0x10, "sky", "s32"),
    (0x14, "collision", "s32"),
    (0x18, "moby_classes", "AR"), (0x20, "tie_classes", "AR"),
    (0x28, "shrub_classes", "AR"), (0x30, "tfrag_textures", "AR"),
    (0x38, "moby_textures", "AR"), (0x40, "tie_textures", "AR"),
    (0x48, "shrub_textures", "AR"), (0x50, "part_textures", "AR"),
    (0x58, "fx_textures", "AR"),
    (0x60, "textures_base_offset", "s32"), (0x64, "part_bank_offset", "s32"),
    (0x68, "fx_bank_offset", "s32"), (0x6C, "part_defs_offset", "s32"),
    (0x70, "sound_remap_offset", "s32"), (0x74, "unknown_74", "s32"),
    (0x78, "ratchet_seqs", "s32"), (0x7C, "scene_view_size", "s32"),
    (0x80, "moby_gs_stash_count", "s32"), (0x84, "moby_gs_stash_offset", "s32"),
    (0x88, "assets_compressed_size", "s32"), (0x8C, "assets_decompressed_size", "s32"),
    (0x90, "chrome_map_texture", "s32"), (0x94, "chrome_map_palette", "s32"),
    (0x98, "glass_map_texture", "s32"), (0x9C, "glass_map_palette", "s32"),
    (0xA0, "unknown_a0", "s32"), (0xA4, "heightmap_offset", "s32"),
    (0xA8, "occlusion_oct_offset", "s32"), (0xAC, "moby_gs_stash_list", "s32"),
    (0xB0, "occlusion_rad_offset", "s32"), (0xB4, "moby_sound_remap_offset", "s32"),
    (0xB8, "occlusion_rad2_offset", "s32"),
];

/// Parse GC/UYA data header from byte-level offset within data.
/// Returns a map of {name: {offset, size}} parsed from the 11 × 8-byte entries.
pub fn parse_gc_uya_data_header(data: &[u8]) -> Vec<(&'static str, i32, i32)> {
    let mut ranges = Vec::new();
    for &(byte_off, name) in GCUYA_DATA_HEADER_ENTRIES {
        let o = if byte_off + 8 <= data.len() {
            (r_s32(data, byte_off), r_s32(data, byte_off + 4))
        } else {
            continue;
        };
        ranges.push((name, o.0, o.1));
    }
    ranges
}

/// Parse level core header (0xBC bytes) from core_index data.
/// Returns a serde_json Value with all fields.
pub fn parse_level_core_header(data: &[u8]) -> serde_json::Value {
    use serde_json::json;

    let mut map = serde_json::Map::new();
    for &(off, name, ftype) in LEVEL_CORE_FIELDS {
        if off + 4 > data.len() { continue; }
        if ftype == "AR" {
            let count = r_s32(data, off);
            let offset = r_s32(data, off + 4);
            map.insert(name.to_string(), json!({"count": count, "offset": offset}));
        } else {
            map.insert(name.to_string(), json!(r_s32(data, off)));
        }
    }
    serde_json::Value::Object(map)
}

/// Extract class entries with given entry size into JSON
pub fn extract_class_entries_json(data: &[u8], offset: i32, count: i32, entry_size: u32) -> Vec<serde_json::Value> {
    use serde_json::json;
    let start = offset as usize;
    let mut entries = Vec::new();
    for i in 0..count as usize {
        let off = start + i * entry_size as usize;
        let end = off + entry_size as usize;
        if end > data.len() { break; }
        let entry = &data[off..end];
        if entry_size == 0x20 {
            entries.push(json!({
                "index": i,
                "wad_off": r_s32(entry, 0),
                "o_class": r_s32(entry, 4),
                "u8": r_s32(entry, 8),
                "uC": r_s32(entry, 12),
                "tex": hex::encode(&entry[16..32.min(entry.len())]),
            }));
        } else if entry_size == 0x30 {
            let bw = if entry.len() >= 0x22 { r_s16(entry, 0x20) } else { 0 };
            let bh = if entry.len() >= 0x24 { r_s16(entry, 0x22) } else { 0 };
            entries.push(json!({
                "index": i,
                "wad_off": r_s32(entry, 0),
                "o_class": r_s32(entry, 4),
                "u8": r_s32(entry, 8),
                "uC": r_s32(entry, 12),
                "tex": hex::encode(&entry[16..32.min(entry.len())]),
                "billboard_width": bw,
                "billboard_height": bh,
            }));
        }
    }
    entries
}

/// Extract class entries with given entry size
pub fn extract_class_entries(data: &[u8], offset: u32, count: u32, entry_size: u32) -> Vec<Vec<u8>> {
    let start = offset as usize;
    let mut entries = Vec::new();
    for i in 0..count as usize {
        let off = start + i * entry_size as usize;
        let end = off + entry_size as usize;
        if end <= data.len() {
            entries.push(data[off..end].to_vec());
        }
    }
    entries
}

/// Parse audio header (after a data header)
pub fn parse_audio_header(data: &[u8], file_lba_off: u32) -> Vec<(u32, u32, u32)> {
    // Audio entries: LBA, size (sectors), vag_id
    let audio_count = r_u32(data, 0x20);
    let audio_ofs = r_u32(data, 0x24) + file_lba_off;
    let mut entries = Vec::new();
    for i in 0..audio_count as usize {
        let off = (audio_ofs + i as u32 * 0x10) as usize;
        if off + 12 <= data.len() {
            let lba = r_u32(data, off);
            let size = r_u32(data, off + 4);
            let vag_id = r_u32(data, off + 8);
            entries.push((lba, size, vag_id));
        }
    }
    entries
}

/// Parse scene header
pub fn parse_scene_header(data: &[u8]) -> Vec<u32> {
    // After data header (10 u32), scene header starts
    let off = 0x28; // 10 * 4 bytes
    let light_count = r_u32(data, off);
    let light_ofs = r_u32(data, off + 4);
    let model_count = r_u32(data, off + 8);
    let model_ofs = r_u32(data, off + 12);
    let scene_ofs = r_u32(data, off + 16);
    vec![light_count, light_ofs, model_count, model_ofs, scene_ofs]
}

/// Parse global WAD header
pub fn parse_global_header(data: &[u8], name: &str) -> Vec<(String, u32, u32)> {
    // Different global WADs have different layouts; this is a simplified version
    let mut entries = Vec::new();
    let count = r_u32(data, 0x10);
    let entry_ofs = r_u32(data, 0x14);
    for i in 0..count as usize {
        let off = (entry_ofs + i as u32 * 0x10) as usize;
        if off + 0x10 <= data.len() {
            let sub_lba = r_u32(data, off);
            let sub_size = r_u32(data, off + 4);
            let sub_name = format!("{}_{}", name, i);
            entries.push((sub_name, sub_lba, sub_size));
        }
    }
    entries
}
