use anyhow::{bail, Result};
pub const MAX_FRAME: usize = 65536;
pub fn validate(kind: u8, size: usize) -> Result<()> {
    if !matches!(kind, b'S' | b'A') || size > MAX_FRAME || (kind == b'A' && !size.is_multiple_of(2))
    {
        bail!("Invalid audio frame");
    }
    Ok(())
}
pub fn encode(kind: u8, payload: &[u8]) -> Result<Vec<u8>> {
    validate(kind, payload.len())?;
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(kind);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
}
impl Decoder {
    pub fn feed(&mut self, data: &[u8]) -> Result<()> {
        if self.pending.len() + data.len() > 2 * MAX_FRAME + 10 {
            bail!("Audio decoder buffer exceeded limit");
        }
        self.pending.extend_from_slice(data);
        Ok(())
    }
    pub fn pop(&mut self) -> Result<Option<(u8, Vec<u8>)>> {
        if self.pending.len() < 5 {
            return Ok(None);
        }
        let kind = self.pending[0];
        let size = u32::from_be_bytes(self.pending[1..5].try_into().unwrap()) as usize;
        validate(kind, size)?;
        if self.pending.len() < size + 5 {
            return Ok(None);
        }
        let bytes = self.pending[5..5 + size].to_vec();
        self.pending.drain(..5 + size);
        Ok(Some((kind, bytes)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_malformed_frames() {
        for (k, n) in [(b'X', 0), (b'A', 1), (b'S', MAX_FRAME + 1)] {
            assert!(validate(k, n).is_err());
        }
    }
    #[test]
    fn arbitrary_frame_boundaries_and_partial_headers() {
        let bytes = b"S\0\0\0\x02{}A\0\0\0\x02\x00\x01";
        let mut decoder = Decoder::default();
        let mut frames = Vec::new();
        for byte in bytes {
            decoder.feed(&[*byte]).unwrap();
            while let Some(frame) = decoder.pop().unwrap() {
                frames.push(frame);
            }
        }
        assert_eq!(frames, vec![(b'S', b"{}".to_vec()), (b'A', vec![0, 1])]);
        decoder.feed(b"A\0\0").unwrap();
        assert!(decoder.pop().unwrap().is_none());
    }
    #[test]
    fn bounded_decoder_rejects_oversized_input() {
        let mut d = Decoder::default();
        assert!(d.feed(&vec![0; MAX_FRAME * 2 + 11]).is_err());
        d.feed(b"A\0\x01\0\x01").unwrap();
        assert!(d.pop().is_err());
    }
}
