#![feature(concat_idents)]
use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

macro_rules! register_reader {
	  ($name:ident, $type:ty, $method:ident) => {
		    pub fn $name<'a>(
            memory: &'a [u8],
            base: usize,
        ) -> impl Iterator<Item = crate::AddressValue<$type>> + 'a {
            Box::new((0..(memory.len() - 3)).filter_map(move |start| {
                Some((
                    base + start,
                    Cursor::new(&memory[start..start + std::mem::size_of::<$type>()])
                        .$method::<NativeEndian>()
                        .ok()?,
                ))
            }))
        }
	  };
}

// pub fn possible_values_i32<'a>(
//     memory: &'a [u8],
//     base: usize,
// ) -> impl Iterator<Item = crate::AddressValue> + 'a {
//     Box::new((0..(memory.len() - 3)).filter_map(move |start| {
//         Some((
//             base + start,
//             Cursor::new(&memory[start..start + std::mem::size_of::<i32>()])
//                 .read_i32::<NativeEndian>()
//                 .ok()?,
//         ))
//     }))
// }

register_reader!(possible_values_i32, i32, read_i32);
register_reader!(possible_values_f32, f32, read_f32);
