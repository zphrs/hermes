use std::fmt::Display;

use crate::helpers::from_fn;

use std::fmt::Debug;

use super::Id;

impl<const N: usize> Id<N> {
    pub(crate) fn display_id_bytes(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> Result<(), std::fmt::Error> {
        let first_zero_offset = self.bytes().len()
            - self
                .bytes()
                .iter()
                .rev()
                .position(|b| *b != 0)
                .unwrap_or(self.bytes().len());
        if first_zero_offset < 2 {
            for byte in &self.bytes()[0..first_zero_offset] {
                write!(f, "{:02X?}", byte)?;
            }
            return Ok(());
        }
        let first_bytes = &self.bytes()[0..2];
        let last_bytes =
            &self.bytes()[(first_zero_offset / 2 - 1) * 2..(first_zero_offset / 2) * 2];
        for byte in first_bytes {
            write!(f, "{:02X?}", byte)?;
        }
        write!(f, "...")?;
        for byte in last_bytes {
            write!(f, "{:02X?}", byte)?;
        }
        Ok(())
    }

    pub(crate) fn debug_id_bytes(
        &self,
        first_zero_offset: usize,
        f: &mut std::fmt::Formatter<'_>,
    ) -> Result<(), std::fmt::Error> {
        let mut chunks_left = first_zero_offset / 8;
        let first_zero_chunk_offset = chunks_left * 8;

        for word in self.bytes()[..first_zero_chunk_offset].chunks(8) {
            // print each word (64 bits) together
            for byte in word {
                write!(f, "{:02X?}", byte)?;
            }
            chunks_left -= 1;
            if f.alternate() && chunks_left != 0 {
                writeln!(f)?;
            }
        }
        Ok(())
    }

    pub(crate) fn get_first_zero_byte(&self) -> usize {
        self.bytes().len()
            - self
                .bytes()
                .iter()
                .rev()
                .position(|b| *b != 0)
                .unwrap_or(self.bytes().len())
    }
}

impl<const N: usize> Debug for Id<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let first_zero_offset = self.get_first_zero_byte();
        if first_zero_offset == 0 {
            return write!(f, "Id()");
        }
        f.debug_tuple("Id")
            .field(&from_fn(|f| self.debug_id_bytes(first_zero_offset, f)))
            .finish()
    }
}

impl<const N: usize> Display for Id<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Id")
            .field(&from_fn(|f| self.display_id_bytes(f)))
            .finish()
    }
}
