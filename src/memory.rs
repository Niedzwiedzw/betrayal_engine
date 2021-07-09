use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use std::ops::{Add, Sub};
use std::{
    io::{Cursor, Read, Write},
    str::FromStr,
};

use std::cmp::{Eq, Ord, PartialEq, PartialOrd};

pub trait ReadFromBytes:
    Default
    + std::fmt::Display
    + std::fmt::Debug
    + Sized
    + FromStr
    + Clone
    + Ord
    + Eq
    + PartialEq
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Copy
    + Sync
    + Send
{
    fn possible_values<'a>(
        reader: &'a [u8],
        base: usize,
    ) -> Box<dyn Iterator<Item = crate::AddressValue<Self>> + 'a>;

    fn read_value(val: Vec<u8>) -> std::io::Result<Self>;
    fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()>;
}

impl ReadFromBytes for u8 {
    fn possible_values<'a>(
        memory: &'a [u8],
        base: usize,
    ) -> Box<dyn Iterator<Item = crate::AddressValue<Self>> + 'a> {
        Box::new(
            (0..(memory.len() - std::mem::size_of::<Self>())).filter_map(move |start| {
                Some((
                    base + start,
                    Cursor::new(&memory[start..start + std::mem::size_of::<Self>()])
                        .read_u8()
                        .ok()?,
                ))
            }),
        )
    }

    fn read_value(val: Vec<u8>) -> std::io::Result<Self> {
        let mut c = std::io::Cursor::new(val);
        Ok(c.read_u8()?)
    }

    fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_u8(*self)?;
        Ok(())
    }
}

macro_rules! read_from_bytes_impl {
    ($SelfT:ty, $method:ident, $write_method:ident) => {
        impl ReadFromBytes for $SelfT {
            fn possible_values<'a>(
                memory: &'a [u8],
                base: usize,
            ) -> Box<dyn Iterator<Item = crate::AddressValue<$SelfT>> + 'a> {
                Box::new(
                    (0..(memory.len() - std::mem::size_of::<$SelfT>())).filter_map(move |start| {
                        Some((
                            base + start,
                            Cursor::new(&memory[start..start + std::mem::size_of::<$SelfT>()])
                                .$method::<NativeEndian>()
                                .ok()?,
                        ))
                    }),
                )
            }

            fn read_value(val: Vec<u8>) -> std::io::Result<Self> {
                let mut c = std::io::Cursor::new(val);
                Ok(c.$method::<NativeEndian>()?)
            }

            fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
                writer.$write_method::<NativeEndian>(*self)?;
                Ok(())
            }
        }
    };
}

read_from_bytes_impl!(i32, read_i32, write_i32);
read_from_bytes_impl!(i16, read_i16, write_i16);
