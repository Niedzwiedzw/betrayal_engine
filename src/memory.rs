use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

pub fn possible_i32_values<'a>(
    memory: &'a [u8],
    base: usize,
) -> impl Iterator<Item = crate::AddressValue> + 'a {
    Box::new((0..(memory.len() - 3)).filter_map(move |start| {
        Some((
            base + start,
            Cursor::new(&memory[start..start + 4])
                .read_i32::<NativeEndian>()
                .ok()?,
        ))
    }))
}
