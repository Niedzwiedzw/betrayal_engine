use crate::{error::BetrayalResult, memory::ReadFromBytes, AddressInfo, ProcessQuery};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, convert::TryInto};

pub fn read_memory<T: ReadFromBytes>(pid: i32, address: usize) -> BetrayalResult<(AddressInfo, T)> {
    ProcessQuery::<T>::new(pid)
        .read_at(pid, address)
        .map(|(info, _address, value)| (info, value))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Field {
    Padding(usize),
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    // F32,
    // F64,
    Pointer32(Box<Self>),
    Pointer64(Box<Self>),
    Struct(ReclassStruct),
    SearchValues(Vec<(Field, String)>),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ValueResult<T> {
    Ok(AddressInfo, T),
    Err(String),
    Padding(usize),
}

impl<T> ValueResult<T> {
    pub fn info(&self) -> Option<&AddressInfo> {
        match self {
            Self::Ok(info, _) => Some(info),
            Self::Err(_) => None,
            Self::Padding(_) => None,
        }
    }
}

impl<T> From<BetrayalResult<(AddressInfo, T)>> for ValueResult<T> {
    fn from(r: BetrayalResult<(AddressInfo, T)>) -> Self {
        match r {
            Ok((info, v)) => Self::Ok(info, v),
            Err(e) => Self::Err(format!("error :: {}", e)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FieldResult {
    Padding(usize),
    U16(ValueResult<u16>),
    I16(ValueResult<i16>),
    U32(ValueResult<u32>),
    I32(ValueResult<i32>),
    U64(ValueResult<u64>),
    I64(ValueResult<i64>),
    U8(ValueResult<u8>),
    F32(ValueResult<f32>),
    F64(ValueResult<f64>),
    Pointer32(usize, Box<Self>),
    Pointer64(usize, Box<Self>),
    ReclassStruct(ReclassResult),
}

impl FieldResult {
    pub fn info(&self) -> Option<&AddressInfo> {
        match self {
            FieldResult::Padding(s) => None,
            FieldResult::U16(r) => r.info(),
            FieldResult::U32(r) => r.info(),
            FieldResult::U64(r) => r.info(),
            FieldResult::I64(r) => r.info(),
            FieldResult::I32(r) => r.info(),
            FieldResult::I16(r) => r.info(),
            FieldResult::U8(r) => r.info(),
            FieldResult::F32(r) => r.info(),
            FieldResult::F64(r) => r.info(),
            FieldResult::Pointer32(_, p) => p.info(),
            FieldResult::Pointer64(_, p) => p.info(),
            FieldResult::ReclassStruct(r) => r
                .fields
                .iter()
                .map(|(_, result)| result)
                .next()
                .map(|s| s.info())
                .flatten(),
        }
    }
}

impl Field {
    pub fn size(&self) -> usize {
        match self {
            Field::Padding(size) => *size,
            Field::I32 => std::mem::size_of::<i32>(),
            Field::I16 => std::mem::size_of::<i16>(),
            Field::U8 => std::mem::size_of::<u8>(),
            // Field::F32 => std::mem::size_of::<f32>(),
            // Field::F64 => std::mem::size_of::<f64>(),
            Field::Pointer32(_) => std::mem::size_of::<u32>(),
            Field::Pointer64(_) => std::mem::size_of::<u64>(),
            Field::Struct(_) => 0,
            Field::U16 => std::mem::size_of::<u16>(),
            Field::U32 => std::mem::size_of::<u32>(),
            Field::I64 => std::mem::size_of::<i64>(),
            Field::U64 => std::mem::size_of::<u64>(),
            Field::SearchValues(v) => 0,
        }
    }

    pub fn result(self, pid: i32, address: usize) -> FieldResult {
        match self {
            Field::Padding(s) => FieldResult::Padding(s),
            Field::U8 => FieldResult::U8(read_memory::<u8>(pid, address).into()),
            Field::I16 => FieldResult::I16(read_memory::<i16>(pid, address).into()),
            Field::U16 => FieldResult::U16(read_memory::<u16>(pid, address).into()),
            Field::I32 => FieldResult::I32(read_memory::<i32>(pid, address).into()),
            Field::U32 => FieldResult::U32(read_memory::<u32>(pid, address).into()),
            Field::I64 => FieldResult::I64(read_memory::<i64>(pid, address).into()),
            Field::U64 => FieldResult::U64(read_memory::<u64>(pid, address).into()),
            // Field::F32 => FieldResult::F32(read_memory::<f32>(pid, address).into()),
            // Field::F64 => FieldResult::F64(read_memory::<f64>(pid, address).into()),
            Field::Pointer32(field) => FieldResult::Pointer32(
                address,
                match read_memory::<u32>(pid, address) {
                    Ok((_info, address)) => {
                        Box::new(field.result(pid, address.try_into().expect("bad platform")))
                    }
                    Err(e) => Box::new(FieldResult::U32(Err(e).into())),
                },
            ),
            Field::Pointer64(field) => FieldResult::Pointer64(
                address,
                match read_memory::<u64>(pid, address) {
                    Ok((_info, address)) => {
                        Box::new(field.result(pid, address.try_into().expect("bad platform")))
                    }
                    Err(e) => Box::new(FieldResult::U64(Err(e).into())),
                },
            ),
            Field::Struct(reclass_struct) => {
                FieldResult::ReclassStruct(reclass_struct.result(pid, address))
            }
            Field::SearchValues(fields) => {
                let mut fields = fields.clone();
                let mut last_result = FieldResult::Padding(0);
                println!(" --- searching ");
                for offset in 0..1000usize {
                    print!(".");
                    let search_address = address + offset;
                    for (_field_idx, (field, value)) in fields.iter().enumerate().rev() {
                        let result = field.clone().result(pid, search_address);
                        match result.compare_value() {
                            Some(v) if &v == value => {
                                println!("\n\nfound! addres: {address} + Padding({offset})\n");
                                return result.into();
                            }
                            _ => {
                                last_result = result;
                            }
                        }
                    }
                }
                last_result.into()
            }
        }
    }
}

impl FieldResult {
    pub fn compare_value(&self) -> Option<String> {
        match self {
            FieldResult::U16(v) => v.compare_value(),
            FieldResult::I16(v) => v.compare_value(),
            FieldResult::U32(v) => v.compare_value(),
            FieldResult::I32(v) => v.compare_value(),
            FieldResult::U64(v) => v.compare_value(),
            FieldResult::I64(v) => v.compare_value(),
            FieldResult::U8(v) => v.compare_value(),
            FieldResult::F32(v) => v.compare_value(),
            FieldResult::F64(v) => v.compare_value(),
            FieldResult::Pointer32(v, _) => Some(v.to_string()),
            FieldResult::Pointer64(v, _) => Some(v.to_string()),
            FieldResult::ReclassStruct(_) => None,
            FieldResult::Padding(_) => None,
        }
    }
}

impl<T: std::fmt::Display> ValueResult<T> {
    pub fn compare_value(&self) -> Option<String> {
        match self {
            ValueResult::Ok(_, v) => Some(v.to_string()),
            ValueResult::Err(_) => None,
            ValueResult::Padding(_) => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReclassStruct {
    pub name: String,
    pub fields: IndexMap<String, Field>,
}

impl ReclassStruct {
    pub fn result(self, pid: i32, address: usize) -> ReclassResult {
        let mut base = address;
        let mut fields = vec![];
        for (name, field) in self.fields {
            let size = field.size();
            fields.push((name, base, field));
            base += size;
        }
        ReclassResult {
            name: self.name,
            fields: fields
                .into_iter()
                .map(|(name, address, field)| {
                    let result = field.result(pid, address);
                    let is_static = result.info().map(|i| i.is_static()).unwrap_or(false);
                    (
                        format!(
                            "[{}{}] :: {}",
                            if is_static { "@" } else { "" },
                            address,
                            name
                        ),
                        result,
                    )
                })
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReclassResult {
    pub name: String,
    pub fields: IndexMap<String, FieldResult>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigEntry {
    pub base_address: String,
    pub struct_definition: ReclassStruct,
}

impl Default for ConfigEntry {
    fn default() -> Self {
        Self {
            base_address: "2137 - 4 * SIZE_I32".to_string(),
            struct_definition: Default::default(),
        }
    }
}

impl ConfigEntry {
    pub fn result(self, pid: i32) -> BetrayalResult<ConfigEntryResult> {
        let base_address = super::scripting::calculate_address(pid, &self.base_address)?;
        Ok(ConfigEntryResult {
            base_address,
            struct_definition: self.struct_definition.result(pid, base_address),
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigEntryResult {
    pub base_address: usize,
    pub struct_definition: ReclassResult,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub entries: Vec<ConfigEntry>,
}

impl Config {
    pub fn result(self, pid: i32) -> BetrayalResult<ConfigResult> {
        Ok(ConfigResult {
            entries: self
                .entries
                .into_iter()
                .map(|e| e.result(pid))
                .collect::<BetrayalResult<_>>()?,
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigResult {
    pub entries: Vec<ConfigEntryResult>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            entries: vec![ConfigEntry::default()],
        }
    }
}

impl Default for ReclassStruct {
    fn default() -> Self {
        Self {
            name: "SomeClass".to_string(),
            fields: (0..5)
                .map(|i| (format!("field_{}", i), Field::I32))
                .chain(std::iter::once((
                    "field_6".to_string(),
                    Field::Pointer32(Box::new(Field::I16)),
                )))
                .chain(std::iter::once((
                    "field_7".to_string(),
                    Field::Pointer64(Box::new(Field::Struct(Self {
                        name: "SomeInnerClass".to_string(),
                        fields: std::iter::once(("field_1".to_string(), Field::U16))
                            .chain(std::iter::once((
                                "field_2".to_string(),
                                Field::Padding(2137),
                            )))
                            .collect(),
                    }))),
                )))
                .collect(),
        }
    }
}
