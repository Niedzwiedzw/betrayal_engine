use {
    byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt},
    ordered_float::OrderedFloat,
    std::{
        cmp::{PartialEq, PartialOrd},
        io::{Cursor, Write},
        ops::{Add, Sub},
        str::FromStr,
    },
};

pub type AddressEntry<T> = (usize, T);

pub trait ReadFromBytes:
    Default + std::fmt::Display + std::fmt::Debug + Sized + FromStr + Clone + PartialEq + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Ord + Copy + Sync + Send
{
    fn possible_values<'a>(reader: &'a [u8], base: usize) -> Box<dyn Iterator<Item = AddressEntry<Self>> + 'a>;

    fn read_value(val: Vec<u8>) -> std::io::Result<Self>;
    fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()>;
}

impl ReadFromBytes for u8 {
    fn possible_values<'a>(memory: &'a [u8], base: usize) -> Box<dyn Iterator<Item = AddressEntry<Self>> + 'a> {
        Box::new((0..(memory.len() - std::mem::size_of::<Self>())).filter_map(move |start| {
            Some((
                base + start,
                Cursor::new(&memory[start..start + std::mem::size_of::<Self>()])
                    .read_u8()
                    .ok()?,
            ))
        }))
    }

    fn read_value(val: Vec<u8>) -> std::io::Result<Self> {
        let mut c = std::io::Cursor::new(val);
        c.read_u8()
    }

    fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_u8(*self)?;
        Ok(())
    }
}

macro_rules! read_from_bytes_impl {
    ($SelfT:ty, $method:ident, $write_method:ident) => {
        impl ReadFromBytes for $SelfT {
            fn possible_values<'a>(memory: &'a [u8], base: usize) -> Box<dyn Iterator<Item = AddressEntry<$SelfT>> + 'a> {
                Box::new((0..(memory.len() - std::mem::size_of::<$SelfT>())).filter_map(move |start| {
                    Some((
                        base + start,
                        Cursor::new(&memory[start..start + std::mem::size_of::<$SelfT>()])
                            .$method::<NativeEndian>()
                            .ok()?,
                    ))
                }))
            }

            fn read_value(val: Vec<u8>) -> std::io::Result<Self> {
                let mut c = std::io::Cursor::new(val);
                c.$method::<NativeEndian>()
            }

            fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
                writer.$write_method::<NativeEndian>(*self)?;
                Ok(())
            }
        }
    };
}

read_from_bytes_impl!(i32, read_i32, write_i32);
read_from_bytes_impl!(u32, read_u32, write_u32);
read_from_bytes_impl!(i64, read_i64, write_i64);
read_from_bytes_impl!(u64, read_u64, write_u64);
read_from_bytes_impl!(i16, read_i16, write_i16);
read_from_bytes_impl!(u16, read_u16, write_u16);
impl ReadFromBytes for OrderedFloat<f32> {
    fn possible_values<'a>(memory: &'a [u8], base: usize) -> Box<dyn Iterator<Item = AddressEntry<OrderedFloat<f32>>> + 'a> {
        Box::new((0..(memory.len() - std::mem::size_of::<f32>())).filter_map(move |start| {
            Some((
                base + start,
                Cursor::new(&memory[start..start + std::mem::size_of::<f32>()])
                    .read_f32::<NativeEndian>()
                    .ok()
                    .map(OrderedFloat)?,
            ))
        }))
    }
    fn read_value(val: Vec<u8>) -> std::io::Result<Self> {
        let mut c = std::io::Cursor::new(val);
        c.read_f32::<NativeEndian>().map(OrderedFloat)
    }
    fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_f32::<NativeEndian>(self.0)?;
        Ok(())
    }
}
impl ReadFromBytes for OrderedFloat<f64> {
    fn possible_values<'a>(memory: &'a [u8], base: usize) -> Box<dyn Iterator<Item = AddressEntry<OrderedFloat<f64>>> + 'a> {
        Box::new((0..(memory.len() - std::mem::size_of::<f64>())).filter_map(move |start| {
            Some((
                base + start,
                Cursor::new(&memory[start..start + std::mem::size_of::<f64>()])
                    .read_f64::<NativeEndian>()
                    .ok()
                    .map(OrderedFloat)?,
            ))
        }))
    }
    fn read_value(val: Vec<u8>) -> std::io::Result<Self> {
        let mut c = std::io::Cursor::new(val);
        c.read_f64::<NativeEndian>().map(OrderedFloat)
    }
    fn write_bytes<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_f64::<NativeEndian>(self.0)?;
        Ok(())
    }
}
