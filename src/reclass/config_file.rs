use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{error::BetrayalResult, memory::ReadFromBytes, ProcessQuery};

pub fn read_memory<T: ReadFromBytes>(pid: i32, address: usize) -> BetrayalResult<T> {
    ProcessQuery::<T>::read_at(pid, address).map(|(_address, value)| value)
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Field {
    I32,
    I16,
    U8,
    F32,
    F64,
    Pointer(usize, Box<Self>),
    Struct(ReclassStruct),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ValueResult<T> {
    Ok(T),
    Err(String),
}

impl<T> From<BetrayalResult<T>> for ValueResult<T> {
    fn from(r: BetrayalResult<T>) -> Self {
        match r {
            Ok(v) => Self::Ok(v),
            Err(e) => Self::Err(format!("error :: {}", e)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FieldResult {
    I32(ValueResult<i32>),
    I16(ValueResult<i16>),
    U8(ValueResult<u8>),
    F32(ValueResult<f32>),
    F64(ValueResult<f64>),
    Pointer(usize, Box<Self>),
    ReclassStruct(ReclassResult),
}

impl Field {
    pub fn size(&self) -> usize {
        match self {
            Field::I32 => std::mem::size_of::<i32>(),
            Field::I16 => std::mem::size_of::<i16>(),
            Field::U8 => std::mem::size_of::<u8>(),
            Field::F32 => std::mem::size_of::<f32>(),
            Field::F64 => std::mem::size_of::<f64>(),
            Field::Pointer(_, _) => std::mem::size_of::<usize>(),
            Field::Struct(_) => 0,
        }
    }

    pub fn result(self, pid: i32, address: usize) -> FieldResult {
        match self {
            Field::I32 => FieldResult::I32(read_memory::<i32>(pid, address).into()),
            Field::I16 => FieldResult::I16(read_memory::<i16>(pid, address).into()),
            Field::U8 => FieldResult::U8(read_memory::<u8>(pid, address).into()),
            Field::F32 => FieldResult::F32(read_memory::<f32>(pid, address).into()),
            Field::F64 => FieldResult::F64(read_memory::<f64>(pid, address).into()),
            Field::Pointer(address, field) => {
                FieldResult::Pointer(address, Box::new(field.result(pid, address)))
            }
            Field::Struct(reclass_struct) => {
                FieldResult::ReclassStruct(reclass_struct.result(pid, address))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReclassStruct {
    pub name: String,
    pub fields: BTreeMap<String, Field>,
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
                    (
                        format!("[{}] :: {}", address, name),
                        field.result(pid, address),
                    )
                })
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReclassResult {
    pub name: String,
    pub fields: BTreeMap<String, FieldResult>,
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
        let base_address = super::scripting::calculate_address(&self.base_address)?;
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
                    Field::Pointer(0x2137, Box::new(Field::I16)),
                )))
                .chain(std::iter::once((
                    "field_7".to_string(),
                    Field::Pointer(
                        0x2139,
                        Box::new(Field::Struct(Self {
                            name: "SomeInnerClass".to_string(),
                            fields: std::iter::once(("field_1".to_string(), Field::F64)).collect(),
                        })),
                    ),
                )))
                .collect(),
        }
    }
}
