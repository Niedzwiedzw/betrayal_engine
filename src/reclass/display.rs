use std::fmt::{Display, Write};

use indexmap::IndexMap;

use super::config_file::{
    ConfigEntryResult, ConfigResult, FieldResult, ReclassResult, ValueResult,
};

pub trait Printable {
    fn print(&self, indent_level: usize) -> String;
}

const INDENTATION: usize = 2;

fn indent(indent_level: usize) -> String {
    std::iter::once(' ')
        .cycle()
        .take(indent_level * INDENTATION)
        .collect::<String>()
}

impl Printable for ReclassResult {
    fn print(&self, indent_level: usize) -> String {
        let mut s = String::new();
        let indent = indent(indent_level);
        write!(s, "{indent}-- {name} -- \n", name = self.name).expect("failed to write");
        write!(s, "{}", self.fields.print(indent_level + 1)).expect("failed to write");
        s
    }
}
type FieldEntry<'a> = (&'a String, &'a FieldResult);
impl Printable for IndexMap<String, FieldResult> {
    fn print(&self, indent_level: usize) -> String {
        self.iter()
            .filter(|(_, f)| match f {
                FieldResult::Padding(_) => false,
                _ => true,
            })
            .map(|v| v.print(indent_level))
            .collect::<Vec<_>>()
            .join("\n")
            .to_string()
    }
}

impl<'a> Printable for FieldEntry<'a> {
    fn print(&self, indent_level: usize) -> String {
        let (name, field) = self;
        let indent = indent(indent_level);
        let field = field.print(0);
        format!("{indent}{name:<12}: {field}")
    }
}

impl Printable for &FieldResult {
    fn print(&self, indent_level: usize) -> String {
        let s = match self {
            FieldResult::Padding(_) => format!("~"),
            FieldResult::U16(v) => format!("(U16) {:<19}", v.print(0)),
            FieldResult::I16(v) => format!("(I16) {:<19}", v.print(0)),
            FieldResult::U32(v) => format!("(U32) {:<19}", v.print(0)),
            FieldResult::I32(v) => format!("(I32) {:<19}", v.print(0)),
            FieldResult::U64(v) => format!("(U64) {:<19}", v.print(0)),
            FieldResult::I64(v) => format!("(I64) {:<19}", v.print(0)),
            FieldResult::U8(v) => format!("( U8) {:<19} ", v.print(0)),
            FieldResult::F32(v) => format!("(F32) {:<19}", v.print(0)),
            FieldResult::F64(v) => format!("(F64) {:<19}", v.print(0)),
            FieldResult::Pointer32(addr, v) => format!("(*{addr}) {:<19}", v.as_ref().print(0)),
            FieldResult::Pointer64(addr, v) => format!("(*{addr}) {:<19}", v.as_ref().print(0)),
            FieldResult::ReclassStruct(s) => s.print(0),
        };
        format!("{}{}", indent(indent_level), s)
    }
}

impl<T: Display> Printable for ValueResult<T> {
    fn print(&self, indent_level: usize) -> String {
        format!(
            "{indent}{value}",
            indent = indent(indent_level),
            value = match self {
                ValueResult::Ok(_, val) => {
                    val.to_string()
                }
                ValueResult::Err(e) => format!("<ERR: {}>", e.to_string()),
                ValueResult::Padding(_) => String::from("~"),
            }
        )
    }
}

impl Printable for ConfigResult {
    fn print(&self, indent_level: usize) -> String {
        self.entries
            .iter()
            .map(|e| e.print(indent_level))
            .collect::<Vec<_>>()
            .join("\n\n")
            .into()
    }
}

impl Printable for ConfigEntryResult {
    fn print(&self, indent_level: usize) -> String {
        let Self {
            base_address,
            struct_definition,
        } = self;
        let indent = indent(indent_level);
        let struct_definition = struct_definition.print(indent_level + 1);
        format!("{indent}:: (*{base_address}) ::\n{struct_definition}")
    }
}
